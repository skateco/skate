use anyhow::anyhow;
use std::error::Error;
use std::io::Write;
use std::process;
use std::process::{Child, Stdio};

pub trait ShellExec {
    fn exec(
        &self,
        command: &str,
        args: &[&str],
        stdin: Option<String>,
    ) -> Result<String, Box<dyn Error>>;
    fn exec_stdout(
        &self,
        command: &str,
        args: &[&str],
        stdin: Option<String>,
    ) -> Result<(), Box<dyn Error>>;
}

#[derive(Clone)]
pub struct RealExec {}

impl ShellExec for RealExec {
    fn exec(
        &self,
        command: &str,
        args: &[&str],
        stdin: Option<String>,
    ) -> Result<String, Box<dyn Error>> {
        let mut cmd = &mut process::Command::new(command);

        cmd = cmd.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());

        if stdin.is_some() {
            cmd = cmd.stdin(Stdio::piped());
        }

        let mut child = cmd.spawn()?;

        Self::write_to_stdin(stdin, &mut child)?;

        let output = child
            .wait_with_output()
            .map_err(|e| anyhow!(e).context("failed to run command"))?;

        if !output.status.success() {
            return Err(anyhow!(
                "{} {} failed, exit code {}, stderr: {}",
                command,
                args.join(" "),
                output.status,
                String::from_utf8_lossy(&output.stderr).to_string()
            )
            .into());
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim_end().into())
    }

    fn exec_stdout(
        &self,
        command: &str,
        args: &[&str],
        stdin: Option<String>,
    ) -> Result<(), Box<dyn Error>> {
        let mut binding = process::Command::new(command);
        let mut cmd = binding
            .args(args)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());

        if stdin.is_some() {
            cmd = cmd.stdin(Stdio::piped());
        }

        let mut child = cmd.spawn()?;

        Self::write_to_stdin(stdin, &mut child)?;

        let output = child
            .wait_with_output()
            .map_err(|e| anyhow!(e).context("failed to run command"))?;

        if !output.status.success() {
            return Err(anyhow!("exit code {}", output.status)
                .context(format!("{} {} failed", command, args.join(" ")))
                .into());
        }

        Ok(())
    }
}

impl RealExec {
    fn write_to_stdin(stdin: Option<String>, child: &mut Child) -> Result<(), Box<dyn Error>> {
        if let Some(input) = stdin {
            let mut child_stdin = child
                .stdin
                .take()
                .ok_or_else(|| anyhow!("Failed to open stdin"))?;

            let input = input.into_bytes();

            child_stdin
                .write_all(&input)
                .expect("Failed to write to stdin");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capture_output() {
        let execer = RealExec {};
        let result = execer.exec("echo", &["Hello, World!"], None);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Hello, World!");
    }

    #[tokio::test]
    async fn test_capture_stdin() {
        let execer = RealExec {};
        let result = execer.exec("cat", &[], Some("Hello, World!".to_string()));
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Hello, World!");
    }

    #[test]
    fn test_exec_stdout() {
        let execer = RealExec {};
        let result = execer.exec_stdout("echo", &["Hello, World!"], None);
        assert!(result.is_ok());
    }
}
