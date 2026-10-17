//! What new Azure machines can be, and what each costs. Settings offers a
//! short list of VM sizes, with any other size typed in, and the sandbox
//! sizes the service accepts. Specs come from the region's own size list and
//! prices from Azure's public Retail Prices API, both read on a worker; a
//! price that cannot be read is left out rather than guessed.
//!
//! Prices are Linux pay-as-you-go list prices in US dollars, before
//! discounts or credits. A VM is billed while it runs and its disk always; a
//! sandbox is billed for CPU and memory while it runs.

use super::{
    Result, Settings,
    http::{self, Client},
};
use crate::cloud::SavedDevice;
use serde::Deserialize;
use std::collections::HashMap;

/// VM sizes Settings lists, small to large; Arm first, as the default is.
pub(crate) const VM_CHOICES: &[&str] = &[
    "Standard_B2pts_v2",
    "Standard_B2pls_v2",
    "Standard_B2ps_v2",
    "Standard_B4ps_v2",
    "Standard_D4ps_v5",
    "Standard_D8ps_v5",
    "Standard_B2s_v2",
    "Standard_D4s_v5",
];

/// Sandbox CPU in millicores. Memory is the most the service allows for it,
/// 2 GiB per core, and the service gives 20 GiB of disk per core.
pub(crate) const SANDBOX_MILLICORES: &[u32] = &[500, 1000, 2000, 4000, 8000];
pub(crate) const DEFAULT_SANDBOX_MILLICORES: u32 = 1000;
const HOURS_PER_MONTH: f64 = 730.0;
const COMPUTE_API_VERSION: &str = "2024-07-01";
const PRICES: &str = "https://prices.azure.com/api/retail/prices";

/// A sandbox size as the service takes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SandboxSize {
    pub(crate) millicores: u32,
    pub(crate) memory_mib: u32,
}

impl SandboxSize {
    /// `millicores` with the most memory the service allows for them.
    pub(crate) fn standard(millicores: u32) -> Self {
        Self {
            millicores,
            memory_mib: millicores * 2048 / 1000,
        }
    }

    /// The size a sandbox reports, as `1000m` and `2048Mi`.
    pub(crate) fn reported(cpu: &str, memory: &str) -> Option<Self> {
        let millicores = cpu.strip_suffix('m')?.parse().ok()?;
        let memory_mib = match memory.strip_suffix("Mi") {
            Some(mib) => mib.parse().ok()?,
            None => memory
                .strip_suffix("Gi")?
                .parse::<u32>()
                .ok()?
                .checked_mul(1024)?,
        };
        Some(Self {
            millicores,
            memory_mib,
        })
    }

    pub(crate) fn disk_gib(self) -> u32 {
        self.millicores * 20 / 1000
    }

    pub(crate) fn cpu(self) -> String {
        format!("{}m", self.millicores)
    }

    pub(crate) fn memory(self) -> String {
        format!("{}Mi", self.memory_mib)
    }

    /// How many cores, as Settings and the config write them: `0.5`, `2`.
    pub(crate) fn cores(self) -> String {
        if self.millicores.is_multiple_of(1000) {
            (self.millicores / 1000).to_string()
        } else {
            format!("{}", f64::from(self.millicores) / 1000.0)
        }
    }

    /// The size for `text` cores, if it is one the service is offered at.
    pub(crate) fn parse(text: &str) -> Option<Self> {
        let cores: f64 = text.trim().parse().ok()?;
        let millicores = (cores * 1000.0).round();
        SANDBOX_MILLICORES
            .iter()
            .find(|&&choice| f64::from(choice) == millicores)
            .map(|&millicores| Self::standard(millicores))
    }

    /// The size as a choice shows it, with the disk the service gives.
    pub(crate) fn summary(self) -> String {
        format!("{} · {} GiB disk", self.short(), self.disk_gib())
    }

    pub(crate) fn short(self) -> String {
        format!(
            "{} vCPU · {}",
            self.cores(),
            memory_text(u64::from(self.memory_mib))
        )
    }
}

/// Whether a VM size runs on Arm: Azure marks Arm sizes with a lowercase `p`
/// among the feature letters after the vCPU count, as in `B2pts_v2`.
pub(crate) fn is_arm(size: &str) -> bool {
    let name = size.strip_prefix("Standard_").unwrap_or(size);
    let name = name.split('_').next().unwrap_or(name);
    let Some(digits) = name.find(|c: char| c.is_ascii_digit()) else {
        return false;
    };
    name[digits..]
        .trim_start_matches(|c: char| c.is_ascii_digit() || c == '-')
        .contains('p')
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub(crate) struct VmSpec {
    #[serde(rename = "numberOfCores")]
    pub(crate) cores: u32,
    #[serde(rename = "memoryInMB")]
    pub(crate) memory_mb: u64,
}

impl VmSpec {
    pub(crate) fn summary(self, size: &str) -> String {
        format!(
            "{} vCPU · {} · {}",
            self.cores,
            memory_text(self.memory_mb),
            if is_arm(size) { "Arm" } else { "x64" }
        )
    }
}

fn memory_text(mib: u64) -> String {
    if mib.is_multiple_of(1024) {
        format!("{} GiB", mib / 1024)
    } else {
        format!("{:.1} GiB", mib as f64 / 1024.0)
    }
}

/// An hourly list price in US dollars.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Price(pub(crate) f64);

impl Price {
    /// `$0.0095/hr · ~$7/mo`, a month being 730 running hours.
    pub(crate) fn text(self, month: bool) -> String {
        let hourly = if self.0 < 1.0 {
            format!("${:.4}/hr", self.0)
        } else {
            format!("${:.2}/hr", self.0)
        };
        if !month {
            return hourly;
        }
        format!("{hourly} · ~${:.0}/mo", self.0 * HOURS_PER_MONTH)
    }
}

/// Container Apps rates that sandboxes are billed at while they run.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SandboxRates {
    pub(crate) vcpu_second: f64,
    pub(crate) gib_second: f64,
}

impl SandboxRates {
    pub(crate) fn hourly(self, size: SandboxSize) -> Price {
        let cores = f64::from(size.millicores) / 1000.0;
        let gib = f64::from(size.memory_mib) / 1024.0;
        Price((cores * self.vcpu_second + gib * self.gib_second) * 3600.0)
    }
}

/// Specs and prices for the configured regions.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Catalog {
    pub(crate) location: String,
    pub(crate) sandbox_location: String,
    /// Every size the VM region offers.
    pub(crate) vms: HashMap<String, VmSpec>,
    pub(crate) vm_prices: HashMap<String, Price>,
    pub(crate) sandbox_rates: Option<SandboxRates>,
}

/// Read what the regions offer and cost. Blocks on the network: call it on a
/// worker. Prices are best effort; specs need the account.
pub(crate) fn catalog(settings: &Settings) -> Result<Catalog> {
    let client = Client::new(settings)?;
    let vms = region_sizes(&client, settings)?;
    let mut sizes: Vec<&str> = VM_CHOICES.to_vec();
    if !sizes.contains(&settings.vm_size.as_str()) && vms.contains_key(&settings.vm_size) {
        sizes.push(&settings.vm_size);
    }
    let vm_prices = vm_prices(&settings.location, &sizes).unwrap_or_else(|error| {
        tracing::warn!(category = "azure_prices", %error, "Could not read Azure VM prices");
        HashMap::new()
    });
    let sandbox_rates = sandbox_rates(&settings.sandbox_location).unwrap_or_else(|error| {
        tracing::warn!(category = "azure_prices", %error, "Could not read Azure sandbox prices");
        None
    });
    Ok(Catalog {
        location: settings.location.clone(),
        sandbox_location: settings.sandbox_location.clone(),
        vms,
        vm_prices,
        sandbox_rates,
    })
}

#[derive(Deserialize)]
struct SizeList {
    #[serde(default)]
    value: Vec<NamedSize>,
}

#[derive(Deserialize)]
struct NamedSize {
    name: String,
    #[serde(flatten)]
    spec: VmSpec,
}

fn region_sizes(client: &Client, settings: &Settings) -> Result<HashMap<String, VmSpec>> {
    let path = format!(
        "/subscriptions/{}/providers/Microsoft.Compute/locations/{}/vmSizes?api-version={COMPUTE_API_VERSION}",
        settings.subscription_id, settings.location
    );
    let list: SizeList = client.get(&path)?;
    Ok(list
        .value
        .into_iter()
        .map(|size| (size.name, size.spec))
        .collect())
}

#[derive(Deserialize)]
struct PriceList {
    #[serde(rename = "Items", default)]
    items: Vec<PriceItem>,
}

#[derive(Clone, Deserialize)]
pub(crate) struct PriceItem {
    #[serde(rename = "armSkuName", default)]
    pub(crate) sku: String,
    #[serde(rename = "skuName", default)]
    pub(crate) sku_name: String,
    #[serde(rename = "productName", default)]
    pub(crate) product: String,
    #[serde(rename = "meterName", default)]
    pub(crate) meter: String,
    #[serde(rename = "retailPrice", default)]
    pub(crate) price: f64,
    #[serde(rename = "currencyCode", default)]
    pub(crate) currency: String,
}

/// Names that can sit inside a quoted OData string as they are.
fn plain(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn vm_prices(location: &str, sizes: &[&str]) -> Result<HashMap<String, Price>> {
    let sizes: Vec<&str> = sizes.iter().copied().filter(|size| plain(size)).collect();
    if sizes.is_empty() || !plain(location) {
        return Ok(HashMap::new());
    }
    let skus = sizes
        .iter()
        .map(|size| format!("armSkuName eq '{size}'"))
        .collect::<Vec<_>>()
        .join(" or ");
    let filter = format!(
        "serviceName eq 'Virtual Machines' and priceType eq 'Consumption' and armRegionName eq '{location}' and ({skus})"
    );
    let list: PriceList = http::get_public(PRICES, &[("$filter", &filter)])?;
    Ok(linux_prices(list.items))
}

/// The Linux pay-as-you-go price of each size: not Windows, Spot, Low
/// Priority, or Cloud Services.
pub(crate) fn linux_prices(items: Vec<PriceItem>) -> HashMap<String, Price> {
    items
        .into_iter()
        .filter(|item| {
            item.currency == "USD"
                && item.product.starts_with("Virtual Machines")
                && !item.product.ends_with("Windows")
                && !item.sku_name.contains("Spot")
                && !item.sku_name.contains("Low Priority")
        })
        .map(|item| (item.sku, Price(item.price)))
        .collect()
}

fn sandbox_rates(location: &str) -> Result<Option<SandboxRates>> {
    if !plain(location) {
        return Ok(None);
    }
    let filter = format!(
        "serviceName eq 'Azure Container Apps' and priceType eq 'Consumption' and armRegionName eq '{location}' and (meterName eq 'Standard vCPU Active Usage' or meterName eq 'Standard Memory Active Usage')"
    );
    let list: PriceList = http::get_public(PRICES, &[("$filter", &filter)])?;
    Ok(rates(&list.items))
}

pub(crate) fn rates(items: &[PriceItem]) -> Option<SandboxRates> {
    let rate = |meter: &str| {
        items
            .iter()
            .find(|item| item.meter == meter && item.currency == "USD")
            .map(|item| item.price)
    };
    Some(SandboxRates {
        vcpu_second: rate("Standard vCPU Active Usage")?,
        gib_second: rate("Standard Memory Active Usage")?,
    })
}

/// Each saved device's size, as Settings lists it, by device ID. Best
/// effort: a device whose machine is gone or unreadable is left out.
pub(crate) fn device_sizes(
    settings: &Settings,
    devices: &[SavedDevice],
    catalog: &Catalog,
) -> HashMap<String, String> {
    use super::machine::Machine;
    let mut sizes = HashMap::new();
    let vms = devices
        .iter()
        .any(|device| matches!(Machine::parse(&device.id), Machine::Vm(_)));
    if vms && let Ok(found) = Client::new(settings).and_then(|client| super::api::vm_sizes(&client))
    {
        for device in devices {
            if let Machine::Vm(name) = Machine::parse(&device.id)
                && let Some(size) = found.get(&name.to_ascii_lowercase())
            {
                let text = match catalog.vms.get(size) {
                    Some(spec) => format!("{size} · {}", spec.summary(size)),
                    None => size.clone(),
                };
                sizes.insert(device.id.clone(), text);
            }
        }
    }
    let sandboxes = devices
        .iter()
        .any(|device| matches!(Machine::parse(&device.id), Machine::Sandbox(_)));
    if sandboxes && let Ok(found) = super::sandbox::sizes(settings) {
        for device in devices {
            if let Machine::Sandbox(id) = Machine::parse(&device.id)
                && let Some(size) = found.get(id)
            {
                sizes.insert(device.id.clone(), size.short());
            }
        }
    }
    sizes
}

#[cfg(test)]
mod tests;
