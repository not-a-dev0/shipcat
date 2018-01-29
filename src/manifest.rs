use serde_yaml;

use std::io::prelude::*;
use std::fs::File;
use std::env;
use std::path::{PathBuf, Path};
use std::collections::{HashMap, BTreeMap};

use super::Result;
use super::vault::Vault;

// k8s related structs

#[derive(Serialize, Deserialize, Clone)]
pub struct ResourceRequest {
    /// CPU request string
    cpu: String,
    /// Memory request string
    memory: String,
    // TODO: ephemeral-storage + extended-resources
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ResourceLimit {
    /// CPU limit string
    cpu: String,
    /// Memory limit string
    memory: String,
    // TODO: ephemeral-storage + extended-resources
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Resources {
    /// Resource requests for k8s
    pub requests: Option<ResourceRequest>,
    /// Resource limits for k8s
    pub limits: Option<ResourceLimit>,
}


#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Replicas {
    /// Minimum replicas for k8s deployment
    pub min: u32,
    /// Maximum replicas for k8s deployment
    pub max: u32,
}


#[derive(Serialize, Deserialize, Clone, Default)]
pub struct ConfigMountedFile {
    /// Name of file to template
    pub name: String,
    /// Name of file as used in code
    pub dest: String,
}
#[derive(Serialize, Deserialize, Clone, Default)]
pub struct ConfigMount {
    /// Optional k8s specific name for the mount (autogenerated if left out)
    pub name: Option<String>,
    /// Container-local path where configs are available
    pub mount: String,
    /// Files from the config map to mount at this mountpath
    pub configs: Vec<ConfigMountedFile>
}

// misc structs

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Dashboard {
    /// Metric strings to track
    pub rows: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Prometheus {
    /// Whether to poll
    pub enabled: bool,
    /// Path to poll
    pub path: String,
    // TODO: Maybe include names of metrics?
}


#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct VaultOpts {
    /// If Vault name differs from service name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// If the secret lives under a special suffix
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suffix: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct HealthCheck {
    // Port where the health check is located (typically first exposed port)
    //pub port: u32,
    // NB: maybe do ports later, currently use first exposed port

    /// Where the health check is located (typically /health)
    pub uri: String,
    /// How long to wait after boot in seconds (typically 30s)
    pub wait: u32
}

//#[derive(Serialize, Clone, Default, Debug)]
//pub struct PortMap {
//    /// Host port
//    pub host: u32,
//    /// Target port
//    pub target: u32,
//}

/// Main manifest, serializable from shipcat.yml
#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Manifest {
    /// Name of the service
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Optional image name (if different from service name)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    /// Optional image command (if not using the default docker command)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,

    // Kubernetes specific flags

    /// Resource limits and requests
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<Resources>,
    /// Replication limits
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replicas: Option<Replicas>,
    /// Environment variables to inject
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<BTreeMap<String, String>>,
    /// Environment files to mount
    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub volumes: Vec<ConfigMount>,
    /// Ports to expose
    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub ports: Vec<u32>,
    /// Vault options
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vault: Option<VaultOpts>,
    /// Health check parameters
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health: Option<HealthCheck>,


    // TODO: boot time -> minReadySeconds

// TODO: service dependencies!

    /// Prometheus metric options
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prometheus: Option<Prometheus>,
//prometheus:
//  enabled: true
//  path: /metrics
    /// Dashboards to generate
    #[serde(default)]
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub dashboards: BTreeMap<String, Dashboard>,
//dashboards:
//  auth-python:
//    rows:
//      - users-connected
//      - conversation-length

// TODO: logging alerts
//logging:
//  alerts:
//    error-rate-5xx:
//      type: median
//      threshold: 2
//      status-code: 500
//preStopHookPath: /die
// newrelic options? we generate the newrelic.ini from a vault secret + manifest.name

    // Internal path of this manifest
    #[serde(skip_serializing, skip_deserializing)]
    _location: String,

//    // Parsed port map of this manifest
//    #[serde(skip_serializing, skip_deserializing)]
//    pub _portmaps: Vec<PortMap>
}


impl Manifest {
    pub fn new(name: &str, location: &PathBuf) -> Manifest {
        Manifest {
            name: Some(name.into()),
            _location: location.to_string_lossy().into(),
            ..Default::default()
        }
    }
    /// Read a manifest file in an arbitrary path
    fn read_from(pwd: &PathBuf) -> Result<Manifest> {
        let mpath = pwd.join("shipcat.yml");
        trace!("Using manifest in {}", mpath.display());
        if !mpath.exists() {
            bail!("Manifest file {} does not exist", mpath.display())
        }
        let mut f = File::open(&mpath)?;
        let mut data = String::new();
        f.read_to_string(&mut data)?;
        let mut res: Manifest = serde_yaml::from_str(&data)?;
        // store the location internally (not serialized to disk)
        res._location = mpath.to_string_lossy().into();
        Ok(res)
    }

    /// Add implicit defaults to self
    fn implicits(&mut self) -> Result<()> {
        let name = self.name.clone().unwrap();

        // image name defaults to the service name
        if self.image.is_none() {
            self.image = Some(name.clone());
        }

        // vault queries by default under
        if self.vault.is_none() {
            self.vault = Some(VaultOpts {
                name: Some(name.clone()),
                suffix: None,
            })
        }
        // volumes get implicit config volume names (for k8s)
        let mut volume_index = 1;
        let num_volumes = self.volumes.len();
        for vol in &mut self.volumes {
            if vol.name.is_none() {
                if volume_index == 1 && num_volumes == 1 {
                    // have simpler config man names when only one volume
                    vol.name = Some(format!("{}-config", name.clone()));
                } else {
                    vol.name = Some(format!("{}-config-{}", name.clone(), volume_index));
                }
                volume_index += 1;
            }
        }

        Ok(())
    }

    /// Merge defaults from partial override file
    fn merge(&mut self, pth: &PathBuf) -> Result<()> {
        trace!("Merging {}", pth.display());
        if !pth.exists() {
            bail!("Defaults file {} does not exist", pth.display())
        }
        let name = self.name.clone().unwrap();
        let mut f = File::open(&pth)?;
        let mut data = String::new();
        f.read_to_string(&mut data)?;
        let mf: Manifest = serde_yaml::from_str(&data)?;


        // TODO: deal with 3 way merges better..
        if let Some(envs) = mf.env {
            let mut mainmap = self.env.clone().unwrap();
            for (k,v) in envs {
                mainmap.entry(k).or_insert(v);
            }
            self.env = Some(mainmap);
        }

        if self.resources.is_none() && mf.resources.is_some() {
            self.resources = mf.resources.clone();
        }
        if let Some(ref mut res) = self.resources {
            if res.limits.is_none() {
                res.limits = mf.resources.clone().unwrap().limits;
            }
            if res.requests.is_none() {
                res.requests = mf.resources.clone().unwrap().requests;
            }
            // for now: if limits or requests are specified, you have to fill in both CPU and memory
        }
        if self.replicas.is_none() && mf.replicas.is_some() {
            self.replicas = mf.replicas;
        }
        if self.ports.is_empty() {
            warn!("{} exposes no ports", name.clone());
        }
        if self.health.is_none() && mf.health.is_some() {
            self.health = mf.health
        }

        // only using target ports now, disabling this now
        //for s in &self.ports {
        //    self._portmaps.push(parse_ports(s)?);
        //}
        Ok(())
    }

    // Populate placeholder fields with secrets from vault
    fn secrets(&mut self, client: &mut Vault, env: &str, loc: &str) -> Result<()> {
        let envmap: HashMap<&str, String> =[
            ("dev", format!("dev-{}", loc)),
        ].iter().cloned().collect();

        if let Some(mut envs) = self.env.clone() {
            // iterate over evar key values and find the ones we need
            for (key, value) in &mut envs {
                if value == "IN_VAULT" {
                    let vopts = self.vault.clone().unwrap();
                    let svc = vopts.name.unwrap();
                    let full_key = format!("{}/{}/{}", envmap[env], svc, key);
                    let secret = client.read(&full_key)?;
                    *value = secret;
                }
            }
            self.env = Some(envs); // overwrite env key with our populated one
        }
        Ok(())
    }

    // Return a completed (read, filled in, and populate secrets) manifest
    pub fn completed(env: &str, loc: &str, service: &str, vault: Option<&mut Vault>) -> Result<Manifest> {
        let pth = Path::new(".").join("services").join(service);
        if !pth.exists() {
            bail!("Service folder {} does not exist", pth.display())
        }
        let mut mf = Manifest::read_from(&pth)?;
        mf.implicits()?;
        if let Some(client) = vault {
            debug!("Injecting secrets from vault {}-{}", env, loc);
            mf.secrets(client, env, loc)?;
        }

        // merge service specific env overrides if they exists
        let envlocals = Path::new(".")
            .join("services")
            .join(service)
            .join(format!("{}-{}.yml", env, loc));
        if envlocals.is_file() {
            debug!("Merging environment locals from {}", envlocals.display());
            mf.merge(&envlocals)?;
        }
        // merge global environment defaults if they exist
        let envglobals = Path::new(".")
            .join("environments")
            .join(format!("{}-{}.yml", env, loc));
        if envglobals.is_file() {
            debug!("Merging environment globals from {}", envglobals.display());
            mf.merge(&envglobals)?;
        }
        Ok(mf)
    }

    /// Update the manifest file in the current folder
    pub fn write(&self) -> Result<()> {
        let encoded = serde_yaml::to_string(self)?;
        trace!("Writing manifest in {}", self._location);
        let mut f = File::create(&self._location)?;
        write!(f, "{}\n", encoded)?;
        debug!("Wrote manifest in {}: \n{}", self._location, encoded);
        Ok(())
    }

    /// Print manifest to stdout
    pub fn print(&self) -> Result<()> {
        let encoded = serde_yaml::to_string(self)?;
        print!("{}\n", encoded);
        Ok(())
    }

    /// Verify assumptions about manifest
    ///
    /// Assumes the manifest has been populated with `implicits`
    pub fn verify(&self) -> Result<()> {
        if self.name.is_none() || self.name.clone().unwrap() == "" {
            bail!("Name cannot be empty")
        }
        let name = self.name.clone().unwrap();

        // 1. Verify resources
        // (We can unwrap all the values as we assume implicit called!)
        let req = self.resources.clone().unwrap().requests.unwrap().clone();
        let lim = self.resources.clone().unwrap().limits.unwrap().clone();
        let req_memory = parse_memory(&req.memory)?;
        let lim_memory = parse_memory(&lim.memory)?;
        let req_cpu = parse_cpu(&req.cpu)?;
        let lim_cpu = parse_cpu(&lim.cpu)?;

        // 1.1 limits >= requests
        if req_cpu > lim_cpu {
            bail!("Requested more CPU than what was limited");
        }
        if req_memory > lim_memory {
            bail!("Requested more memory than what was limited");
        }
        // 1.2 sanity numbers
        if req_cpu > 10.0 {
            bail!("Requested more than 10 cores");
        }
        if req_memory > 10.0*1024.0*1024.0*1024.0 {
            bail!("Requested more than 10 GB of memory");
        }
        if lim_cpu > 20.0 {
            bail!("CPU limit set to more than 20 cores");
        }
        if lim_memory > 20.0*1024.0*1024.0*1024.0 {
            bail!("Memory limit set to more than 20 GB of memory");
        }

        // 2. Ports restrictions? currently parse only

        // 3. volumes
        // 3.1) mount paths can't be empty string
        for v in &self.volumes {
            if v.mount == "" || v.mount == "~" {
                bail!("Empty mountpath for {} mount ", v.name.clone().unwrap())
            }
        }
        if self.volumes.len() > 1 {
            bail!("{} using more than one config volume", name.clone());
        }

        // X. TODO: other keys

        Ok(())
    }
}

// Parse normal k8s memory resource value into floats
fn parse_memory(s: &str) -> Result<f64> {
    let digits = s.chars().take_while(|ch| ch.is_digit(10) || *ch == '.').collect::<String>();
    let unit = s.chars().skip_while(|ch| ch.is_digit(10) || *ch == '.').collect::<String>();
    let mut res : f64 = digits.parse()?;
    trace!("Parsed {} ({})", digits, unit);
    if unit == "Ki" {
        res *= 1024.0;
    } else if unit == "Mi" {
        res *= 1024.0*1024.0;
    } else if unit == "Gi" {
        res *= 1024.0*1024.0*1024.0;
    } else if unit == "k" {
        res *= 1000.0;
    } else if unit == "M" {
        res *= 1000.0*1000.0;
    } else if unit == "G" {
        res *= 1000.0*1000.0*1000.0;
    } else if unit != "" {
        bail!("Unknown unit {}", unit);
    }
    trace!("Returned {} bytes", res);
    Ok(res)
}

// Parse normal k8s cpu resource values into floats
// We don't allow power of two variants here
fn parse_cpu(s: &str) -> Result<f64> {
    let digits = s.chars().take_while(|ch| ch.is_digit(10) || *ch == '.').collect::<String>();
    let unit = s.chars().skip_while(|ch| ch.is_digit(10) || *ch == '.').collect::<String>();
    let mut res : f64 = digits.parse()?;

    trace!("Parsed {} ({})", digits, unit);
    if unit == "m" {
        res /= 1000.0;
    } else if unit == "k" {
        res *= 1000.0;
    } else if unit != "" {
        bail!("Unknown unit {}", unit);
    }
    trace!("Returned {} cores", res);
    Ok(res)
}

pub fn validate(env: &str, location: &str, service: &str) -> Result<()> {
    let mf = Manifest::completed(env, location, service, None)?;
    mf.verify()?;
    mf.print()
}

pub fn init() -> Result<()> {
    let pwd = env::current_dir()?;
    let last_comp = pwd.components().last().unwrap(); // std::path::Component
    let dirname = last_comp.as_os_str().to_str().unwrap();

    let mf = Manifest::new(dirname, &pwd.join("shipcat.yml"));
    mf.write()
}
