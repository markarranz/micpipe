use std::path::PathBuf;
use std::process::Command as ProcCommand;

use crate::cli::RunArgs;

const SERVICE_NAME: &str = "micpipe";
const SERVICE_LABEL: &str = "com.markarranz.micpipe";
const PLIST_TEMPLATE: &str = include_str!("plist.template");

pub fn install(args: RunArgs) {
    // Ensure the log directory exists (StandardOutPath won't create it).
    std::fs::create_dir_all(log_dir()).expect("could not create log dir");

    // Build ProgramArguments: the binary, "run", and the baked-in options.
    let mut program_args = vec![
        binary_path().to_str().unwrap().to_string(),
        "run".to_string(),
        "--output".to_string(),
        args.output.clone(),
    ];
    if let Some(input) = &args.input {
        program_args.push("--input".to_string());
        program_args.push(input.clone());
    }
    if args.debug {
        program_args.push("--debug".to_string());
    }

    let args_xml: String = program_args
        .iter()
        .map(|a| format!("        <string>{}</string>", xml_escape(a)))
        .collect::<Vec<String>>()
        .join("\n");

    let out_log = log_dir().join("out.log");
    let err_log = log_dir().join("err.log");

    let plist = render_plist(
        SERVICE_LABEL,
        &args_xml,
        out_log.to_str().unwrap(),
        err_log.to_str().unwrap(),
    );
    std::fs::write(plist_path(), plist).expect("could not write plist");

    // Bootstrap it.
    let status = ProcCommand::new("launchctl")
        .args([
            "bootstrap",
            &domain_target(),
            plist_path().to_str().unwrap(),
        ])
        .status()
        .expect("failed to run launchctl");

    if status.success() {
        println!("{} service installed and started", SERVICE_NAME);
    } else {
        eprintln!(
            "plist written, but bootstrap failed (it may already be loaded - try `{} restart`",
            SERVICE_NAME
        );
    }
}

pub fn uninstall() {
    let _ = ProcCommand::new("launchctl")
        .args(["bootout", &service_target()])
        .output();

    if plist_path().exists() {
        std::fs::remove_file(plist_path()).expect("could not remove plist");
        println!("Removed {}", plist_path().display());
    }

    println!("{} service uninstalled", SERVICE_NAME);
}

pub fn start() {
    todo!("launchctl bootstrap")
}
pub fn stop() {
    todo!("launchctl bootout")
}
pub fn restart() {
    todo!("launchctl kickstart")
}
pub fn status() {
    todo!("launchctl print + parse")
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").expect("HOME not set"))
}

fn plist_path() -> PathBuf {
    home()
        .join("Library/LaunchAgents")
        .join(format!("{}.plist", SERVICE_LABEL))
}

fn binary_path() -> PathBuf {
    std::env::current_exe().expect("could not determine current executable path")
}

fn log_dir() -> PathBuf {
    home().join(format!(".local/share/{}", SERVICE_NAME))
}

fn current_uid() -> String {
    let output = std::process::Command::new("id")
        .arg("-u")
        .output()
        .expect("failed to run `id`");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn service_target() -> String {
    format!("gui/{}/{}", current_uid(), SERVICE_LABEL)
}

fn domain_target() -> String {
    format!("gui/{}", current_uid())
}

/// Escapes the characters that must be escaped inside XML *element text*
/// (&, <, >). Not sufficient for attribute values, which also need quotes escaped.
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn render_plist(label: &str, program_args_xml: &str, out_log: &str, err_log: &str) -> String {
    PLIST_TEMPLATE
        .replace("{{LABEL}}", label)
        .replace("{{PROGRAM_ARGS}}", program_args_xml)
        .replace("{{OUT_LOG}}", out_log)
        .replace("{{ERR_LOG}}", err_log)
}
