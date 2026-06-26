use std::path::PathBuf;
use std::process::Command as ProcCommand;

use crate::{
    cli::RunArgs,
    error::{self, Result, ResultExt},
};

const SERVICE_NAME: &str = "micpipe";
const SERVICE_LABEL: &str = "com.markarranz.micpipe";
const PLIST_TEMPLATE: &str = include_str!("plist.template");

pub fn install(args: RunArgs) -> Result<()> {
    let log_dir = log_dir()?;
    std::fs::create_dir_all(&log_dir)
        .context(format!("could not create log dir {}", log_dir.display()))?;

    let mut program_args = vec![
        binary_path()?.to_string_lossy().into_owned(),
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

    let out_log = log_dir.join("out.log");
    let err_log = log_dir.join("err.log");

    let plist = render_plist(
        SERVICE_LABEL,
        &args_xml,
        out_log.to_string_lossy().as_ref(),
        err_log.to_string_lossy().as_ref(),
    );
    let plist_path = plist_path()?;
    std::fs::write(&plist_path, plist)
        .context(format!("could not write plist {}", plist_path.display()))?;

    let domain = domain_target()?;
    let status = ProcCommand::new("launchctl")
        .args([
            "bootstrap",
            domain.as_str(),
            plist_path.to_string_lossy().as_ref(),
        ])
        .status()
        .context("failed to run launchctl bootstrap")?;

    if status.success() {
        println!("{} service installed and started", SERVICE_NAME);
    } else {
        return Err(error::message(format!(
            "plist written, but bootstrap failed (it may already be loaded - try `{} restart`)",
            SERVICE_NAME
        )));
    }
    Ok(())
}

pub fn uninstall() -> Result<()> {
    let _ = ProcCommand::new("launchctl")
        .args(["bootout", service_target()?.as_str()])
        .output();

    let plist_path = plist_path()?;
    if plist_path.exists() {
        std::fs::remove_file(&plist_path)
            .context(format!("could not remove plist {}", plist_path.display()))?;
        println!("Removed {}", plist_path.display());
    }

    println!("{} service uninstalled", SERVICE_NAME);
    Ok(())
}

pub fn start() -> Result<()> {
    let plist_path = plist_path()?;
    if !plist_path.exists() {
        return Err(error::message(format!(
            "Service not installed. Run `{} install` first.",
            SERVICE_NAME
        )));
    }

    let domain = domain_target()?;
    let status = ProcCommand::new("launchctl")
        .args([
            "bootstrap",
            domain.as_str(),
            plist_path.to_string_lossy().as_ref(),
        ])
        .status()
        .context("failed to run launchctl bootstrap")?;

    if status.success() {
        println!("{} started.", SERVICE_NAME);
    } else {
        return Err(error::message(
            "Failed to start (it may already be running).",
        ));
    }
    Ok(())
}

pub fn stop() -> Result<()> {
    let domain = domain_target()?;
    let plist_path = plist_path()?;
    let status = ProcCommand::new("launchctl")
        .args([
            "bootout",
            domain.as_str(),
            plist_path.to_string_lossy().as_ref(),
        ])
        .status()
        .context("failed to run launchctl bootout")?;

    if status.success() {
        println!("{} stopped.", SERVICE_NAME);
    } else {
        return Err(error::message(
            "Failed to stop (it may not have been running).",
        ));
    }
    Ok(())
}

pub fn restart() -> Result<()> {
    let status = restart_service()?;
    if status.success() {
        println!("{} restarted.", SERVICE_NAME);
    } else {
        return Err(error::message(
            "Failed to restart (is it installed and loaded?).",
        ));
    }
    Ok(())
}

pub fn restart_service() -> Result<std::process::ExitStatus> {
    let target = service_target()?;
    ProcCommand::new("launchctl")
        .args(["kickstart", "-k", target.as_str()])
        .status()
        .context("failed to run launchctl kickstart")
}

pub fn status() -> Result<()> {
    let plist_path = plist_path()?;
    if !plist_path.exists() {
        println!("not installed");
        println!("run `{} install` to set up the service", SERVICE_NAME);
        return Ok(());
    }

    let target = service_target()?;
    let output = ProcCommand::new("launchctl")
        .args(["print", target.as_str()])
        .output()
        .context("failed to run launchctl print")?;
    if !output.status.success() {
        println!("installed but not loaded");
        println!("plist: {}", plist_path.display());
        println!("run `{} start` to load it", SERVICE_NAME);
        return Ok(());
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let pid = text
        .lines()
        .find_map(|l| l.trim().strip_prefix("pid = "))
        .map(|p| p.trim());
    let last_exit = text
        .lines()
        .find_map(|l| l.trim().strip_prefix("last exit code = "))
        .map(|c| c.trim());

    match pid {
        Some(pid) => println!("running (pid {})", pid),
        None => {
            print!("loaded, but not running");
            if let Some(code) = last_exit {
                print!(" (last exit code {})", code);
            }
            println!();
        }
    }

    println!("plist: {}", plist_path.display());
    println!("logs: {}", log_dir()?.display());
    Ok(())
}

fn home() -> Result<PathBuf> {
    Ok(PathBuf::from(
        std::env::var("HOME").context("HOME not set")?,
    ))
}

fn plist_path() -> Result<PathBuf> {
    Ok(home()?
        .join("Library/LaunchAgents")
        .join(format!("{}.plist", SERVICE_LABEL)))
}

fn binary_path() -> Result<PathBuf> {
    std::env::current_exe().context("could not determine current executable path")
}

fn log_dir() -> Result<PathBuf> {
    Ok(home()?.join(format!(".local/share/{}", SERVICE_NAME)))
}

fn current_uid() -> Result<String> {
    let output = std::process::Command::new("id")
        .arg("-u")
        .output()
        .context("failed to run `id -u`")?;
    if !output.status.success() {
        return Err(error::message("failed to determine current uid"));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn service_target() -> Result<String> {
    Ok(format!("gui/{}/{}", current_uid()?, SERVICE_LABEL))
}

fn domain_target() -> Result<String> {
    Ok(format!("gui/{}", current_uid()?))
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

#[cfg(test)]
mod tests {
    use super::{render_plist, xml_escape};

    #[test]
    fn escapes_xml_element_text() {
        assert_eq!(xml_escape("A&B < C > D"), "A&amp;B &lt; C &gt; D");
    }

    #[test]
    fn renders_launch_agent_plist() {
        let plist = render_plist(
            "com.example.micpipe",
            "        <string>micpipe</string>",
            "/tmp/out.log",
            "/tmp/err.log",
        );

        assert!(plist.contains("<string>com.example.micpipe</string>"));
        assert!(plist.contains("        <string>micpipe</string>"));
        assert!(plist.contains("<string>/tmp/out.log</string>"));
        assert!(plist.contains("<string>/tmp/err.log</string>"));
    }
}
