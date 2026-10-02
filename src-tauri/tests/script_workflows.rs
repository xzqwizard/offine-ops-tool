use offline_preops_tool_lib::{
    builder::{render, template_env},
    io_util,
};
use serde_json::{json, Value};
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

fn bash() -> Option<String> {
    if cfg!(windows) {
        let p = "D:/Environment/Git/usr/bin/bash.exe";
        Path::new(p).exists().then(|| p.into())
    } else {
        Some("/bin/bash".into())
    }
}
fn ctx(root: &Path, build: &str) -> Value {
    json!({"project_name":"$(touch SHOULD_NOT_EXIST)","project_id":"proj-test","server_id":"srv-test","stack_name":"test-stack","build_id":build,"baseline_build_id":if build=="b-old" {""} else {"b-old"},"firewall_chain":"OPO_test","server":{"deploy_base_dir":posix(&root.join("operation data")),"ip":"127.0.0.1","arch":"amd64","os_family":"ubuntu","docker_version":"27.5.1","docker_data_root":"/data/docker"},"instances":[{"instance_name":"demo","health_cmd":"'sh' '-c' 'true'","health_timeout":1,"image":"demo:v1","data_volume":"/data","ports":[]}],"images":[]})
}
fn write_package(root: &Path, context: &Value) {
    fs::create_dir_all(root.join("scripts")).unwrap();
    fs::create_dir_all(root.join("stack/data/demo")).unwrap();
    fs::create_dir_all(root.join("docker-offline")).unwrap();
    fs::create_dir_all(root.join("docs")).unwrap();
    fs::write(root.join("README.md"), "test").unwrap();
    let env = template_env().unwrap();
    for n in ["runtime", "deploy", "upgrade", "ops", "apply-firewall"] {
        fs::write(
            root.join(format!("scripts/{n}.sh")),
            render(&env, &format!("scripts/{n}.sh.j2"), context).unwrap(),
        )
        .unwrap();
    }
    fs::write(
        root.join("stack/docker-compose.yml"),
        "services:\n  demo:\n    image: demo:v1\n",
    )
    .unwrap();
    fs::write(root.join("stack/data/demo/value"), "original-data").unwrap();
    fs::write(root.join("manifest.json"), "{}").unwrap();
    fs::write(
        root.join("deployment.env"),
        format!(
            "PROJECT_ID='proj-test'\nSERVER_ID='srv-test'\nBUILD_ID='{}'\n",
            context["build_id"].as_str().unwrap()
        ),
    )
    .unwrap();
    fs::write(root.join("scripts/precheck.sh"), "#!/bin/bash\nexit 0\n").unwrap();
    fs::write(
        root.join("docker-offline/install-docker.sh"),
        "#!/bin/bash\nprintf 'installer\\n' >> \"$MOCK_LOG\"\n",
    )
    .unwrap();
    sums(root);
}
fn sums(root: &Path) {
    let mut entries = vec![];
    collect(root, root, &mut entries);
    entries.sort();
    fs::write(root.join("SHA256SUMS"), entries.join("\n") + "\n").unwrap();
}
fn collect(root: &Path, p: &Path, out: &mut Vec<String>) {
    for e in fs::read_dir(p).unwrap() {
        let path = e.unwrap().path();
        if path.is_dir() {
            collect(root, &path, out);
        } else if !["SHA256SUMS", ".installed", ".ops.lock"]
            .contains(&path.file_name().unwrap().to_str().unwrap())
            && !path.starts_with(root.join("logs"))
        {
            out.push(format!(
                "{}  {}",
                io_util::sha256_file(&path).unwrap(),
                path.strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            ));
        }
    }
}
fn mocks(bin: &Path) {
    fs::create_dir_all(bin).unwrap();
    let docker = r#"#!/usr/bin/env bash
set -eu
printf '%s\n' "$*" >> "$MOCK_LOG"
if [ "$1" = compose ]; then
 shift; file=''
 while [ "$#" -gt 0 ]; do case "$1" in --project-directory|-p) shift 2;; -f) file=$2; shift 2;; *) break;; esac; done
 cmd=$1; shift
 case "$cmd" in
 config) if [ "${1:-}" = --images ]; then echo demo:v1; fi;;
 ps) if [[ "$*" = *--services* ]]; then echo demo; else echo container-id; fi;;
 up) if grep -q CANDIDATE_FAIL "$file"; then exit 1; fi;;
 exec) exit 0;;
 esac
 exit 0
fi
if [ "$1" = image ] && [ "$2" = inspect ] && [[ "$*" = *'.Size'* ]]; then echo 1024; exit 0; fi
if [ "$1" = image ] && [ "$2" = inspect ]; then printf 'sha256:%064d\n' 0; exit 0; fi
if [ "$1" = inspect ] && [[ "$*" = *'.State.ExitCode'* ]]; then echo 0; exit 0; fi
if [ "$1" = image ] && [ "$2" = save ]; then
  echo mock-image-tar > "$4"
  if [ -f "$MOCK_LOG.fail-save" ]; then exit 1; fi
  exit 0
fi
exit 0
"#;
    for (name, body) in [
        ("docker", docker),
        ("id", "#!/bin/bash\necho 0\n"),
        ("flock", "#!/bin/bash\nexit 0\n"),
    ] {
        let p = bin.join(name);
        fs::write(&p, body).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
}
fn posix(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        format!("/{}/{}", s[..1].to_ascii_lowercase(), &s[3..])
    } else {
        s
    }
}
fn run(bash: &str, bin: &Path, log: &Path, script: &Path, args: &[&str]) -> Output {
    let mut command = Command::new(bash);
    // Windows prevents renaming a directory that contains an open script handle.
    // Feed the entry script as code with its real $0; Linux exercises the regular file path.
    if cfg!(windows) {
        command.arg("-c").arg(fs::read_to_string(script).unwrap());
    }
    command
        .arg(posix(script))
        .args(args.iter().map(|s| {
            if cfg!(windows) && s.as_bytes().get(1) == Some(&b':') {
                posix(Path::new(s))
            } else {
                s.to_string()
            }
        }))
        .env("PATH", format!("{}:{}", posix(bin), "/usr/bin:/bin"))
        .env("MOCK_LOG", posix(log))
        .output()
        .unwrap()
}
fn assert_ok(o: Output) {
    assert!(
        o.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
}

#[test]
fn deployed_backup_restore_and_upgrade_failure_return_to_original_data_and_version() {
    let Some(bash) = bash() else { return };
    let t = io_util::Cleanup(
        std::env::temp_dir().join(format!("preops-shell-{}", uuid::Uuid::new_v4().simple())),
    );
    fs::create_dir_all(&t.0).unwrap();
    let bin = t.0.join("bin");
    mocks(&bin);
    let log = t.0.join("commands.log");
    let deployed = t.0.join("deployed");
    write_package(&deployed, &ctx(&t.0, "b-old"));
    assert_ok(run(
        &bash,
        &bin,
        &log,
        &deployed.join("scripts/deploy.sh"),
        &[],
    ));
    assert!(fs::read_to_string(&log).unwrap().contains("installer"));
    assert!(!deployed.join("SHOULD_NOT_EXIST").exists());
    fs::write(format!("{}.fail-save", log.display()), b"1").unwrap();
    let failed = run(
        &bash,
        &bin,
        &log,
        &deployed.join("scripts/ops.sh"),
        &["backup"],
    );
    assert!(!failed.status.success());
    let backup_root = t.0.join("operation data/backups/proj-test/srv-test");
    assert!(!fs::read_dir(&backup_root).unwrap().any(|entry| entry
        .unwrap()
        .path()
        .extension()
        .is_some_and(|e| e == "pending")));
    fs::remove_file(format!("{}.fail-save", log.display())).unwrap();
    let upgrade = t.0.join("upgrade");
    write_package(&upgrade, &ctx(&t.0, "b-new"));
    let target = deployed.to_string_lossy().into_owned();
    assert_ok(run(
        &bash,
        &bin,
        &log,
        &upgrade.join("scripts/upgrade.sh"),
        &["--target", &target, "--dry-run"],
    ));
    assert_eq!(
        fs::read_to_string(deployed.join("deployment.env"))
            .unwrap()
            .lines()
            .last()
            .unwrap(),
        "BUILD_ID='b-old'"
    );
    fs::write(
        upgrade.join("stack/docker-compose.yml"),
        "# CANDIDATE_FAIL\nservices:\n  demo:\n    image: demo:v2\n",
    )
    .unwrap();
    sums(&upgrade);
    let result = run(
        &bash,
        &bin,
        &log,
        &upgrade.join("scripts/upgrade.sh"),
        &["--target", &target],
    );
    assert!(!result.status.success());
    assert!(fs::read_to_string(deployed.join("deployment.env"))
        .unwrap()
        .contains("BUILD_ID='b-old'"));
    assert_eq!(
        fs::read_to_string(deployed.join("stack/data/demo/value")).unwrap(),
        "original-data"
    );
    let snapshot = fs::read_dir(&backup_root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.join("COMPLETE").exists())
        .unwrap();
    fs::write(deployed.join("stack/data/demo/value"), "new-data").unwrap();
    assert_ok(run(
        &bash,
        &bin,
        &log,
        &deployed.join("scripts/ops.sh"),
        &["restore", &snapshot.to_string_lossy(), "--yes"],
    ));
    assert_eq!(
        fs::read_to_string(deployed.join("stack/data/demo/value")).unwrap(),
        "original-data"
    );
    fs::write(snapshot.join("snapshot.tar.gz"), "corrupt").unwrap();
    let result = run(
        &bash,
        &bin,
        &log,
        &deployed.join("scripts/ops.sh"),
        &["restore", &snapshot.to_string_lossy(), "--yes"],
    );
    assert!(!result.status.success());
    assert_eq!(
        fs::read_to_string(deployed.join("stack/data/demo/value")).unwrap(),
        "original-data"
    );
}

#[test]
fn upgrade_refuses_itself_and_wrong_baseline_before_stopping_services() {
    let Some(bash) = bash() else { return };
    let t = io_util::Cleanup(
        std::env::temp_dir().join(format!("preops-upgrade-{}", uuid::Uuid::new_v4().simple())),
    );
    fs::create_dir_all(&t.0).unwrap();
    let bin = t.0.join("bin");
    mocks(&bin);
    let log = t.0.join("commands.log");
    let package = t.0.join("package");
    write_package(&package, &ctx(&t.0, "b-new"));
    let out = run(
        &bash,
        &bin,
        &log,
        &package.join("scripts/upgrade.sh"),
        &["--target", &package.to_string_lossy()],
    );
    assert!(!out.status.success());
    let wrong = t.0.join("wrong");
    write_package(&wrong, &ctx(&t.0, "b-wrong"));
    let out = run(
        &bash,
        &bin,
        &log,
        &package.join("scripts/upgrade.sh"),
        &["--target", &wrong.to_string_lossy()],
    );
    assert!(!out.status.success());
    assert!(!log.exists() || !fs::read_to_string(log).unwrap().contains(" stop"));
}
