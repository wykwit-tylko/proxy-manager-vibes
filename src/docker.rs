use std::process::Command;

use anyhow::{Context, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkInfo {
    pub name: String,
    pub driver: String,
    pub containers: usize,
    pub scope: String,
}

pub trait DockerRuntime {
    fn list_containers(&self, all: bool) -> Result<Vec<String>>;
    fn list_networks(&self) -> Result<Vec<NetworkInfo>>;
    fn container_network(&self, container: &str) -> Result<Option<String>>;
    fn build_image(&self, tag: &str, build_dir: &str) -> Result<()>;
    fn container_exists(&self, name: &str) -> Result<bool>;
    fn container_status(&self, name: &str) -> Result<Option<String>>;
    fn run_container(&self, name: &str, image: &str, network: &str, ports: &[u16]) -> Result<()>;
    fn stop_remove_container(&self, name: &str) -> Result<()>;
    fn connect_network(&self, name: &str, network: &str) -> Result<()>;
    fn container_logs(&self, name: &str, tail: usize, follow: bool) -> Result<Vec<String>>;
    fn create_network(&self, name: &str) -> Result<()>;
}

pub struct CliDocker;

impl CliDocker {
    fn run_command(args: &[&str]) -> Result<String> {
        let output = Command::new("docker")
            .args(args)
            .output()
            .with_context(|| format!("Failed to execute docker {:?}", args))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("docker {:?} failed: {}", args, stderr));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    fn network_container_count(name: &str) -> Result<usize> {
        let output = Self::run_command(&[
            "network",
            "inspect",
            name,
            "--format",
            "{{len .Containers}}",
        ])?;
        Ok(output.trim().parse::<usize>().unwrap_or(0))
    }
}

impl DockerRuntime for CliDocker {
    fn list_containers(&self, all: bool) -> Result<Vec<String>> {
        let mut args = vec!["ps", "--format", "{{.Names}}"];
        if all {
            args.insert(1, "-a");
        }
        let output = Self::run_command(&args)?;
        if output.is_empty() {
            return Ok(Vec::new());
        }
        Ok(output.lines().map(|l| l.trim().to_string()).collect())
    }

    fn list_networks(&self) -> Result<Vec<NetworkInfo>> {
        let output = Self::run_command(&[
            "network",
            "ls",
            "--format",
            "{{.Name}}|{{.Driver}}|{{.Scope}}|",
        ])?;
        if output.is_empty() {
            return Ok(Vec::new());
        }
        let mut networks = Vec::new();
        for line in output.lines() {
            let mut parts = line.split('|');
            let name = parts.next().unwrap_or("").to_string();
            let driver = parts.next().unwrap_or("").to_string();
            let scope = parts.next().unwrap_or("").to_string();
            if name.is_empty() {
                continue;
            }
            let containers = Self::network_container_count(&name).unwrap_or(0);
            networks.push(NetworkInfo {
                name,
                driver,
                containers,
                scope,
            });
        }
        Ok(networks)
    }

    fn container_network(&self, container: &str) -> Result<Option<String>> {
        let output = Command::new("docker")
            .args([
                "inspect",
                container,
                "--format",
                "{{json .NetworkSettings.Networks}}",
            ])
            .output()
            .with_context(|| format!("Failed to inspect container {}", container))?;
        if !output.status.success() {
            return Ok(None);
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        if stdout.trim().is_empty() || stdout.trim() == "null" {
            return Ok(None);
        }
        let value: serde_json::Value = serde_json::from_str(stdout.trim())?;
        if let Some(obj) = value.as_object()
            && let Some(name) = obj.keys().next()
        {
            return Ok(Some(name.to_string()));
        }
        Ok(None)
    }

    fn build_image(&self, tag: &str, build_dir: &str) -> Result<()> {
        Self::run_command(&["build", "-t", tag, build_dir]).map(|_| ())
    }

    fn container_exists(&self, name: &str) -> Result<bool> {
        let output = Command::new("docker")
            .args(["ps", "-a", "--format", "{{.Names}}"])
            .output()
            .with_context(|| "Failed to list containers")?;
        if !output.status.success() {
            return Ok(false);
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(stdout.lines().any(|l| l.trim() == name))
    }

    fn container_status(&self, name: &str) -> Result<Option<String>> {
        let output = Command::new("docker")
            .args(["inspect", name, "--format", "{{.State.Status}}"])
            .output()
            .with_context(|| format!("Failed to inspect container {}", name))?;
        if !output.status.success() {
            return Ok(None);
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let status = stdout.trim();
        if status.is_empty() {
            Ok(None)
        } else {
            Ok(Some(status.to_string()))
        }
    }

    fn run_container(&self, name: &str, image: &str, network: &str, ports: &[u16]) -> Result<()> {
        let mut args: Vec<String> = vec![
            "run".to_string(),
            "-d".to_string(),
            "--name".to_string(),
            name.to_string(),
            "--network".to_string(),
            network.to_string(),
        ];
        for port in ports {
            args.push("-p".to_string());
            args.push(format!("{}:{}", port, port));
        }
        args.push(image.to_string());
        let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        Self::run_command(&args_ref).map(|_| ())
    }

    fn stop_remove_container(&self, name: &str) -> Result<()> {
        let _ = Self::run_command(&["rm", "-f", name]);
        Ok(())
    }

    fn connect_network(&self, name: &str, network: &str) -> Result<()> {
        let _ = Self::run_command(&["network", "connect", network, name]);
        Ok(())
    }

    fn container_logs(&self, name: &str, tail: usize, follow: bool) -> Result<Vec<String>> {
        let mut args: Vec<String> =
            vec!["logs".to_string(), "--tail".to_string(), tail.to_string()];
        if follow {
            args.push("-f".to_string());
        }
        args.push(name.to_string());
        let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        let output = Self::run_command(&args_ref)?;
        Ok(output.lines().map(|l| l.to_string()).collect())
    }

    fn create_network(&self, name: &str) -> Result<()> {
        Self::run_command(&["network", "create", name]).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeDocker {
        containers: Vec<String>,
    }

    impl DockerRuntime for FakeDocker {
        fn list_containers(&self, _all: bool) -> Result<Vec<String>> {
            Ok(self.containers.clone())
        }
        fn list_networks(&self) -> Result<Vec<NetworkInfo>> {
            Ok(vec![])
        }
        fn container_network(&self, _container: &str) -> Result<Option<String>> {
            Ok(None)
        }
        fn build_image(&self, _tag: &str, _build_dir: &str) -> Result<()> {
            Ok(())
        }
        fn container_exists(&self, _name: &str) -> Result<bool> {
            Ok(false)
        }
        fn container_status(&self, _name: &str) -> Result<Option<String>> {
            Ok(None)
        }
        fn run_container(
            &self,
            _name: &str,
            _image: &str,
            _network: &str,
            _ports: &[u16],
        ) -> Result<()> {
            Ok(())
        }
        fn stop_remove_container(&self, _name: &str) -> Result<()> {
            Ok(())
        }
        fn connect_network(&self, _name: &str, _network: &str) -> Result<()> {
            Ok(())
        }
        fn container_logs(&self, _name: &str, _tail: usize, _follow: bool) -> Result<Vec<String>> {
            Ok(vec![])
        }
        fn create_network(&self, _name: &str) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn fake_list_containers() {
        let docker = FakeDocker {
            containers: vec!["a".to_string(), "b".to_string()],
        };
        assert_eq!(docker.list_containers(true).unwrap().len(), 2);
    }
}
