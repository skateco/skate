use std::error::Error;
use std::process;
use anyhow::anyhow;

pub trait ShellExec {
    fn exec(&self, command: &str, args: &[&str]) -> Result<String, Box<dyn Error>>;
    fn exec_stdout(&self, command: &str, args: &[&str]) -> Result<(), Box<dyn Error>>;
}


#[derive(Clone)]
pub struct RealExec {}

impl ShellExec for RealExec {
    fn exec(&self, command: &str, args: &[&str]) -> Result<String, Box<dyn Error>> {
        let output = process::Command::new(command)
            .args(args)
            .output().map_err(|e| anyhow!(e).context("failed to run command"))?;
        if !output.status.success() {
            return Err(anyhow!("exit code {}, stderr: {}", output.status, String::from_utf8_lossy(&output.stderr).to_string()).context(format!("{} {} failed", command, args.join(" "))).into());
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim_end().into())
    }

    fn exec_stdout(&self, command: &str, args: &[&str]) -> Result<(), Box<dyn Error>> {
        let output = process::Command::new(command)
            .args(args)
            .stdout(process::Stdio::inherit())
            .stderr(process::Stdio::inherit())
            .status().map_err(|e| anyhow!(e).context("failed to run command"))?;
        if !output.success() {
            return Err(anyhow!("exit code {}", output).context(format!("{} {} failed", command, args.join(" "))).into());
        }

        Ok(())
    }
}