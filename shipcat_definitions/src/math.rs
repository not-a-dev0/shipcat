use super::structs::Resources;
use super::structs::rollingupdate::{RollingUpdate};
use super::{Result, Manifest};

/// Total resource usage for a Manifest
///
/// Accounting for workers, replicas, sidecars, and autoscaling policies for these.
#[derive(Serialize, Default)]
pub struct ResourceTotals {
    /// Sum of basic resource structs (ignoring autoscaling limits)
    pub base: Resources<f64>,
    /// Autoscaling Ceilings on top of required
    pub extra: Resources<f64>,
}

/// Calculations done based on values in manifests
///
/// These generally assume that `verify` has passed on all manifests.
impl Manifest {


    /// Estimate how many iterations needed in a kube rolling upgrade
    ///
    /// Used to `estimate_wait_time` for a rollout.
    fn estimate_rollout_iterations(&self) -> u32 {
        let rcount = if let Some(ref hpa) = self.autoScaling {
            hpa.minReplicas
        } else {
            self.replicaCount.unwrap() // verify ensures we have one of these
        };
        if let Some(ru) = self.rollingUpdate.clone() {
            ru.rollout_iterations(rcount)
        } else {
            RollingUpdate::default().rollout_iterations(rcount)
        }
    }

    /// Estimate how long to wait for a kube rolling upgrade
    ///
    /// Was used by helm, now used by the internal upgrade wait time.
    pub fn estimate_wait_time(&self) -> u32 {
        // TODO: handle install case elsewhere..
        if let Some(size) = self.imageSize {
            // 512 default => extra 90s wait, then 90s per half gig...
            // TODO: smoothen..
            let pulltimeestimate = std::cmp::max(60, ((size as f64 * 90.0) / 512.0) as u32);
            let rollout_iterations = self.estimate_rollout_iterations();
            //println!("estimating wait for {} cycle rollout: size={} (est={})", rollout_iterations, size, pulltimeestimate);

            // how long each iteration needs to wait due to readinessProbe params.
            let delayTime = (if let Some(ref hc) = self.health {
                hc.wait
            } else if let Some(ref rp) = self.readinessProbe {
                rp.initialDelaySeconds
            } else {
                30 // guess value in weird case where no health / readiessProbe
            } as f64 * 1.5).ceil() as u32; // give it some leeway
            // leeway scales linearly with wait because we assume accuracy goes down..

            // Final formula: (how long to wait to poll + how long to pull) * num cycles
            (delayTime + pulltimeestimate) * rollout_iterations
        } else {
            warn!("Missing imageSize in {}", self.name);
            300 // helm default --timeout value
        }
    }

    /// Compute the total resource usage of a service
    ///
    /// This relies on the `Mul` and `Add` implementations of `Resources<f64>`,
    /// which allows us to do `+` and `*` on a normalised Resources struct.
    pub fn compute_resource_totals(&self) -> Result<ResourceTotals> {
        let mut base : Resources<f64> = Resources::default();
        let mut extra : Resources<f64> = Resources::default(); // autoscaling limits
        let res = self.resources.clone().unwrap().normalised()?; // exists by verify
        if let Some(ref ascale) = self.autoScaling {
            base = base + (res.clone() * ascale.minReplicas);
            extra = extra + (res.clone() * (ascale.maxReplicas - ascale.minReplicas));
        }
        else if let Some(rc) = self.replicaCount {
            // can trust the replicaCount here
            base = base + (res.clone() * rc);
            for s in &self.sidecars {
                if let Some(ref scrsc) = s.resources {
                    // sidecar replicaCount == main deployment replicaCount
                    base = base + scrsc.normalised()? * rc;
                }
                // TODO: mandatory? sidecar resources when using sidecars?
            }
        } else {
            bail!("{} does not have replicaCount", self.name);
        }
        for w in &self.workers {
            base = base + (w.resources.normalised()? * w.replicaCount);
            // TODO: account for autoscaling in workers when it's there

            // NB: workers get the same sidecars!
            for s in &self.sidecars {
                if let Some(ref scrsc) = s.resources {
                    // worker sidecar replicaCount == worker deployment replicaCount
                    base = base + scrsc.normalised()? * w.replicaCount;
                }
                // TODO: mandatory? sidecar resources when using sidecars?
            }

        }
        Ok(ResourceTotals { base, extra })
    }

}


#[cfg(test)]
mod tests {
    use crate::structs::HealthCheck;
    use super::{Manifest};

    #[test]
    fn mf_wait_time_check() {
        // standard setup - 300s wait is helm default
        let mut mf = Manifest::default();
        mf.imageSize = Some(512);
        mf.health = Some(HealthCheck {
            uri: "/".into(),
            wait: 60,
            ..Default::default()
        });
        mf.replicaCount = Some(2);
        assert_eq!(mf.estimate_wait_time(), 180); // 60*1.5 + 90s
        mf.replicaCount = Some(3); // needs two cycles now
        assert_eq!(mf.estimate_wait_time(), 360); // (60*1.5 + 90s)*2

        // huge image, fast boot
        // causes some pretty high numbers atm - mostly there to catch variance
        // this factor can be scaled down in the future
        mf.imageSize = Some(4096);
        mf.health = Some(HealthCheck {
            uri: "/".into(),
            wait: 10,
            ..Default::default()
        });
        mf.replicaCount = Some(2);
        assert_eq!(mf.estimate_wait_time(), 735); // very high.. network not always reliable

        // medium images, sloooow boot
        mf.imageSize = Some(512);
        mf.health = Some(HealthCheck {
            uri: "/".into(),
            wait: 600,
            ..Default::default()
        });
        mf.replicaCount = Some(2);
        assert_eq!(mf.estimate_wait_time(), 990); // lots of leeway here just in case

    }
}
