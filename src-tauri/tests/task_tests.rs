use offline_preops_tool_lib::tasks;
use std::{
    process::Command,
    time::{Duration, Instant},
};

fn long_command() -> Command {
    if cfg!(windows) {
        let mut cmd = Command::new("powershell.exe");
        cmd.args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Start-Sleep -Seconds 30",
        ]);
        cmd
    } else {
        let mut cmd = Command::new("/bin/sleep");
        cmd.arg("30");
        cmd
    }
}
#[test]
fn subprocess_deadline_terminates_and_reaps_the_child() {
    let start = Instant::now();
    let error = tasks::output(&mut long_command(), Duration::from_millis(100), None).unwrap_err();
    assert!(error.to_string().contains("已终止"));
    assert!(start.elapsed() < Duration::from_secs(5));
}

#[test]
fn deadline_covers_stalled_stdin_and_descendant_output_pipes() {
    let start = Instant::now();
    let input = vec![b'x'; 2 * 1024 * 1024];
    assert!(tasks::output(
        &mut long_command(),
        Duration::from_millis(100),
        Some(&input)
    )
    .unwrap_err()
    .to_string()
    .contains("已终止"));
    assert!(start.elapsed() < Duration::from_secs(5));
    let mut cmd = if cfg!(windows) {
        let mut cmd = Command::new("powershell.exe");
        cmd.args(["-NoProfile", "-NonInteractive", "-Command", "Start-Process -WindowStyle Hidden -FilePath powershell.exe -ArgumentList '-NoProfile -NonInteractive -Command Start-Sleep -Seconds 30'; Start-Sleep -Seconds 30"]);
        cmd
    } else {
        let mut cmd = Command::new("/bin/sh");
        cmd.args(["-c", "sleep 30 & sleep 30"]);
        cmd
    };
    let start = Instant::now();
    assert!(tasks::output(&mut cmd, Duration::from_millis(500), None)
        .unwrap_err()
        .to_string()
        .contains("已终止"));
    assert!(start.elapsed() < Duration::from_secs(5));
}
#[test]
fn running_and_queued_jobs_can_be_cancelled() {
    let id = uuid::Uuid::new_v4().to_string();
    let _session = tasks::Session::begin(Some(id.clone())).unwrap();
    assert!(tasks::Session::begin(Some(id.clone())).is_err());
    let cancel = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        tasks::cancel_task(id).unwrap();
    });
    let error = tasks::output(&mut long_command(), Duration::from_secs(10), None).unwrap_err();
    cancel.join().unwrap();
    assert!(error.to_string().contains("任务已取消"));
    drop(_session);
    let id = uuid::Uuid::new_v4().to_string();
    tasks::cancel_task(id.clone()).unwrap();
    let _session = tasks::Session::begin(Some(id)).unwrap();
    assert!(tasks::check().is_err());
}
