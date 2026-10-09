//! Rewrite two real Docker archives sharing a tag into independently loadable images.
use offline_preops_tool_lib::{image_archive, images};
use std::{fs, path::Path};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 5 {
        return Err("usage: render_identity_pair DIR IMAGE_A_TAR IMAGE_B_TAR REFERENCE".into());
    }
    let root = Path::new(&args[1]);
    fs::create_dir_all(root)?;
    let mut rows = vec![];
    for (name, source) in [("a", &args[2]), ("b", &args[3])] {
        let verified = image_archive::verify(Path::new(source), &args[4], "linux", "amd64")?;
        let reference = images::delivery_reference(&verified.config_digest)?;
        images::rewrite_archive(
            Path::new(source),
            &root.join(format!("{name}.tar")),
            &reference,
        )?;
        rows.push(format!("{name}\t{}\t{reference}", verified.config_digest));
    }
    if rows[0].split('\t').nth(1) == rows[1].split('\t').nth(1) {
        return Err("fixture images must contain different configs".into());
    }
    fs::write(root.join("images.tsv"), rows.join("\n") + "\n")?;
    Ok(())
}
