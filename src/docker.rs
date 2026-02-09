use anyhow::{Context, bail};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkInfo {
    pub name: String,
    pub driver: String,
    pub containers: usize,
    pub scope: String,
}

pub trait DockerApi {
    fn list_containers(&self, filter: Option<&str>) -> anyhow::Result<Vec<String>>;
    fn list_network_names(&self) -> anyhow::Result<Vec<String>>;
    fn create_network(&self, name: &str) -> anyhow::Result<()>;
    fn inspect_container_network(&self, name: &str) -> anyhow::Result<Option<String>>;
    fn list_networks(&self) -> anyhow::Result<Vec<NetworkInfo>>;
    fn build_image(&self, path: &str, tag: &str) -> anyhow::Result<()>;
    fn container_exists(&self, name: &str) -> anyhow::Result<bool>;
    fn run_proxy(
        &self,
        image: &str,
        name: &str,
        network: &str,
        host_ports: &[u16],
    ) -> anyhow::Result<()>;
    fn connect_network(&self, network: &str, container: &str) -> anyhow::Result<()>;
    fn stop_and_remove(&self, name: &str) -> anyhow::Result<bool>;
    fn inspect_status(&self, name: &str) -> anyhow::Result<Option<String>>;
    fn logs(&self, name: &str, follow: bool, tail: usize) -> anyhow::Result<()>;
}

#[derive(Debug, Default, Clone)]
pub struct DockerCli;

impl DockerCli {
    fn run_capture(args: &[&str]) -> anyhow::Result<String> {
        let out = Command::new("docker")
            .args(args)
            .output()
            .with_context(|| format!("Failed to execute docker {:?}", args))?;

        if out.status.success() {
            return Ok(String::from_utf8_lossy(&out.stdout).to_string());
        }

        let stderr = String::from_utf8_lossy(&out.stderr);
        bail!("docker {:?} failed: {}", args, stderr.trim())
    }

    fn run_status(args: &[&str]) -> anyhow::Result<bool> {
        let status = Command::new("docker")
            .args(args)
            .status()
            .with_context(|| format!("Failed to execute docker {:?}", args))?;
        Ok(status.success())
    }
}

impl DockerApi for DockerCli {
    fn list_containers(&self, filter: Option<&str>) -> anyhow::Result<Vec<String>> {
        let output = Self::run_capture(&["ps", "-a", "--format", "{{.Names}}"])?;
        let mut names: Vec<String> = output
            .lines()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned)
            .collect();
        if let Some(f) = filter {
            let needle = f.to_ascii_lowercase();
            names.retain(|n| n.to_ascii_lowercase().contains(&needle));
        }
        Ok(names)
    }

    fn list_network_names(&self) -> anyhow::Result<Vec<String>> {
        let output = Self::run_capture(&["network", "ls", "--format", "{{.Name}}"])?;
        Ok(output
            .lines()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned)
            .collect())
    }

    fn create_network(&self, name: &str) -> anyhow::Result<()> {
        let ok = Self::run_status(&["network", "create", name])?;
        if !ok {
            bail!("Failed to create network {name}");
        }
        Ok(())
    }

    fn inspect_container_network(&self, name: &str) -> anyhow::Result<Option<String>> {
        let output = Self::run_capture(&[
            "inspect",
            "-f",
            "{{range $k,$v := .NetworkSettings.Networks}}{{$k}}{{\"\\n\"}}{{end}}",
            name,
        ]);
        match output {
            Ok(value) => Ok(value
                .lines()
                .map(str::trim)
                .find(|line| !line.is_empty())
                .map(ToOwned::to_owned)),
            Err(err) => {
                if err.to_string().contains("No such object") {
                    Ok(None)
                } else {
                    Err(err)
                }
            }
        }
    }

    fn list_networks(&self) -> anyhow::Result<Vec<NetworkInfo>> {
        let output = Self::run_capture(&[
            "network",
            "ls",
            "--format",
            "{{.Name}}|{{.Driver}}|{{.Scope}}",
        ])?;

        let mut items = Vec::new();
        for line in output.lines().map(str::trim).filter(|l| !l.is_empty()) {
            let parts: Vec<&str> = line.split('|').collect();
            if parts.len() != 3 {
                continue;
            }
            let name = parts[0].to_string();
            let inspect =
                Self::run_capture(&["network", "inspect", "-f", "{{len .Containers}}", &name])
                    .unwrap_or_else(|_| "0".to_string());
            let containers = inspect.trim().parse::<usize>().unwrap_or(0);
            items.push(NetworkInfo {
                name,
                driver: parts[1].to_string(),
                scope: parts[2].to_string(),
                containers,
            });
        }

        Ok(items)
    }

    fn build_image(&self, path: &str, tag: &str) -> anyhow::Result<()> {
        let ok = Self::run_status(&["build", "-t", tag, path])?;
        if !ok {
            bail!("Docker image build failed for tag {tag}");
        }
        Ok(())
    }

    fn container_exists(&self, name: &str) -> anyhow::Result<bool> {
        let ok = Self::run_status(&["inspect", name])?;
        Ok(ok)
    }

    fn run_proxy(
        &self,
        image: &str,
        name: &str,
        network: &str,
        host_ports: &[u16],
    ) -> anyhow::Result<()> {
        let mut cmd = Command::new("docker");
        cmd.arg("run")
            .arg("-d")
            .arg("--name")
            .arg(name)
            .arg("--network")
            .arg(network);
        for port in host_ports {
            let mapping = format!("{port}:{port}");
            cmd.arg("-p").arg(mapping);
        }
        cmd.arg(image);
        let status = cmd.status().context("Failed to run docker proxy")?;
        if !status.success() {
            bail!("Failed to start proxy container {name}");
        }
        Ok(())
    }

    fn connect_network(&self, network: &str, container: &str) -> anyhow::Result<()> {
        let ok = Self::run_status(&["network", "connect", network, container])?;
        if !ok {
            bail!("Failed to connect {container} to network {network}");
        }
        Ok(())
    }

    fn stop_and_remove(&self, name: &str) -> anyhow::Result<bool> {
        if !self.container_exists(name)? {
            return Ok(false);
        }
        let stop_ok = Self::run_status(&["stop", name])?;
        let rm_ok = Self::run_status(&["rm", name])?;
        if stop_ok && rm_ok {
            Ok(true)
        } else {
            bail!("Failed to stop/remove container {name}")
        }
    }

    fn inspect_status(&self, name: &str) -> anyhow::Result<Option<String>> {
        let output = Self::run_capture(&["inspect", "-f", "{{.State.Status}}", name]);
        match output {
            Ok(v) => Ok(Some(v.trim().to_string())),
            Err(err) => {
                if err.to_string().contains("No such object") {
                    Ok(None)
                } else {
                    Err(err)
                }
            }
        }
    }

    fn logs(&self, name: &str, follow: bool, tail: usize) -> anyhow::Result<()> {
        let mut cmd = Command::new("docker");
        cmd.arg("logs").arg("--tail").arg(tail.to_string());
        if follow {
            cmd.arg("-f");
        }
        cmd.arg(name);
        let status = cmd.status().context("Failed to execute docker logs")?;
        if !status.success() {
            bail!("Failed to show logs for {name}");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn network_split_line_parser_shape() {
        let line = "bridge|bridge|local";
        let parts: Vec<&str> = line.split('|').collect();
        assert_eq!(parts, vec!["bridge", "bridge", "local"]);
    }

    #[test]
    fn filtering_is_case_insensitive() {
        let names = ["Foo-App", "bar-app", "baz"];
        let needle = "APP".to_ascii_lowercase();
        let filtered: Vec<&str> = names
            .iter()
            .copied()
            .filter(|n| n.to_ascii_lowercase().contains(&needle))
            .collect();
        assert_eq!(filtered, vec!["Foo-App", "bar-app"]);
    }
}
