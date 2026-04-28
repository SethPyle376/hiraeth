use std::{
    env,
    path::PathBuf,
    process::{Command, ExitStatus, Stdio},
};

fn main() {
    println!("cargo:rerun-if-changed=assets/app.tailwind.css");
    println!("cargo:rerun-if-changed=templates");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=package.json");
    println!("cargo:rerun-if-changed=package-lock.json");

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let node_modules_dir = manifest_dir.join("node_modules");

    ensure_supported_node_version(&manifest_dir);

    if !node_modules_dir.exists() {
        install_web_dependencies(&manifest_dir);
    }

    if !run_command(&manifest_dir, "npm", &["run", "build:css"]).success() {
        eprintln!(
            "css build failed; refreshing npm dependencies to recover optional native bindings"
        );
        install_web_dependencies(&manifest_dir);

        let retry_status = run_command(&manifest_dir, "npm", &["run", "build:css"]);
        if !retry_status.success() {
            panic!(
                "`npm run build:css` failed in {} with status {retry_status}",
                manifest_dir.display()
            );
        }
    }
}

fn install_web_dependencies(cwd: &PathBuf) {
    let install_status = run_command(cwd, "npm", &["install", "--include=optional"]);
    if !install_status.success() {
        panic!(
            "`npm install --include=optional` failed in {} with status {install_status}",
            cwd.display()
        );
    }
}

fn ensure_supported_node_version(cwd: &PathBuf) {
    let output = Command::new("node")
        .arg("--version")
        .current_dir(cwd)
        .stdin(Stdio::null())
        .output()
        .unwrap_or_else(|error| {
            panic!(
                "failed to execute `node --version` in {}: {error}",
                cwd.display()
            )
        });

    if !output.status.success() {
        panic!(
            "`node --version` failed in {} with status {}",
            cwd.display(),
            output.status
        );
    }

    let version = String::from_utf8_lossy(&output.stdout);
    let major = version
        .trim()
        .trim_start_matches('v')
        .split('.')
        .next()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or_else(|| {
            panic!(
                "unable to parse Node.js version output `{}`",
                version.trim()
            )
        });

    if major < 20 {
        panic!(
            "Node.js 20 or newer is required to build hiraeth_web assets; found {}",
            version.trim()
        );
    }
}

fn run_command(cwd: &PathBuf, program: &str, args: &[&str]) -> ExitStatus {
    Command::new(program)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .status()
        .unwrap_or_else(|error| {
            panic!(
                "failed to execute `{program} {}` in {}: {error}",
                args.join(" "),
                cwd.display()
            )
        })
}
