//! Azure Resource Manager REST API calls for Virtual Machines and resources.

use super::{Error, Result, http::Client};
use serde::Deserialize;
use std::time::{Duration, Instant};

const API_VERSION_RESOURCES: &str = "2021-04-01";
const API_VERSION_COMPUTE: &str = "2023-09-01";
const API_VERSION_NETWORK: &str = "2023-09-01";
const POLL_INTERVAL: Duration = Duration::from_secs(3);
const TIMEOUT: Duration = Duration::from_secs(10 * 60);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PowerState {
    Running,
    Stopped,
    Deallocated,
    Starting,
    Unknown,
}

#[derive(Clone, Debug, Deserialize)]
struct VmSummary {
    name: String,
    #[serde(default)]
    properties: Option<VmProperties>,
}

#[derive(Clone, Debug, Deserialize)]
struct VmProperties {
    #[serde(rename = "hardwareProfile", default)]
    hardware: Option<HardwareProfile>,
}

#[derive(Clone, Debug, Deserialize)]
struct HardwareProfile {
    #[serde(rename = "vmSize", default)]
    size: Option<String>,
}

#[derive(Deserialize)]
struct VmList {
    #[serde(default)]
    value: Vec<VmSummary>,
}

#[derive(Deserialize)]
struct InstanceView {
    #[serde(default)]
    statuses: Vec<StatusItem>,
}

#[derive(Deserialize)]
struct StatusItem {
    #[serde(default)]
    code: String,
}

#[derive(Deserialize)]
struct PublicIpResponse {
    properties: Option<PublicIpProperties>,
}

#[derive(Deserialize)]
struct PublicIpProperties {
    #[serde(rename = "ipAddress")]
    ip_address: Option<String>,
}

/// Create the resource group in `location` if it is missing. An existing
/// group is left where it is: Azure refuses to move one, and its resources
/// may live in any region.
pub(crate) fn ensure_resource_group(client: &Client, rg: &str, location: &str) -> Result<()> {
    let path = format!(
        "/subscriptions/{}/resourcegroups/{}?api-version={}",
        client.settings.subscription_id, rg, API_VERSION_RESOURCES
    );
    match client.get::<serde_json::Value>(&path) {
        Ok(_) => return Ok(()),
        Err(Error::Status(404, _)) => {}
        Err(error) => return Err(error),
    }
    let body = serde_json::json!({
        "location": location
    });
    let _: serde_json::Value = client.put(&path, &body)?;
    Ok(())
}

pub(crate) fn deploy_vm(client: &Client, vm_name: &str, ssh_public_key: &str) -> Result<()> {
    ensure_resource_group(
        client,
        &client.settings.resource_group,
        &client.settings.location,
    )?;

    let is_arm = super::sizes::is_arm(&client.settings.vm_size);

    let (publisher, offer, sku) = if is_arm {
        (
            "Canonical",
            "0001-com-ubuntu-server-jammy",
            "22_04-lts-arm64",
        )
    } else {
        (
            "Canonical",
            "0001-com-ubuntu-server-jammy",
            "22_04-lts-gen2",
        )
    };

    let template = serde_json::json!({
        "$schema": "https://schema.management.azure.com/schemas/2019-04-01/deploymentTemplate.json#",
        "contentVersion": "1.0.0.0",
        "resources": [
            {
                "type": "Microsoft.Network/networkSecurityGroups",
                "apiVersion": API_VERSION_NETWORK,
                "name": format!("{}-nsg", vm_name),
                "location": client.settings.location,
                "properties": {
                    "securityRules": [
                        {
                            "name": "SSH",
                            "properties": {
                                "protocol": "Tcp",
                                "sourcePortRange": "*",
                                "destinationPortRange": "22",
                                "sourceAddressPrefix": "*",
                                "destinationAddressPrefix": "*",
                                "access": "Allow",
                                "priority": 1000,
                                "direction": "Inbound"
                            }
                        }
                    ]
                }
            },
            {
                "type": "Microsoft.Network/virtualNetworks",
                "apiVersion": API_VERSION_NETWORK,
                "name": format!("{}-vnet", vm_name),
                "location": client.settings.location,
                "properties": {
                    "addressSpace": {
                        "addressPrefixes": ["10.0.0.0/16"]
                    },
                    "subnets": [
                        {
                            "name": "default",
                            "properties": {
                                "addressPrefix": "10.0.0.0/24"
                            }
                        }
                    ]
                }
            },
            {
                "type": "Microsoft.Network/publicIPAddresses",
                "apiVersion": API_VERSION_NETWORK,
                "name": format!("{}-ip", vm_name),
                "location": client.settings.location,
                "sku": { "name": "Standard" },
                "properties": {
                    "publicIPAllocationMethod": "Static"
                }
            },
            {
                "type": "Microsoft.Network/networkInterfaces",
                "apiVersion": API_VERSION_NETWORK,
                "name": format!("{}-nic", vm_name),
                "location": client.settings.location,
                "dependsOn": [
                    format!("[resourceId('Microsoft.Network/publicIPAddresses', '{}-ip')]", vm_name),
                    format!("[resourceId('Microsoft.Network/virtualNetworks', '{}-vnet')]", vm_name),
                    format!("[resourceId('Microsoft.Network/networkSecurityGroups', '{}-nsg')]", vm_name)
                ],
                "properties": {
                    "ipConfigurations": [
                        {
                            "name": "ipconfig1",
                            "properties": {
                                "privateIPAllocationMethod": "Dynamic",
                                "publicIPAddress": {
                                    "id": format!("[resourceId('Microsoft.Network/publicIPAddresses', '{}-ip')]", vm_name)
                                },
                                "subnet": {
                                    "id": format!("[resourceId('Microsoft.Network/virtualNetworks/subnets', '{}-vnet', 'default')]", vm_name)
                                }
                            }
                        }
                    ],
                    "networkSecurityGroup": {
                        "id": format!("[resourceId('Microsoft.Network/networkSecurityGroups', '{}-nsg')]", vm_name)
                    }
                }
            },
            {
                "type": "Microsoft.Compute/virtualMachines",
                "apiVersion": API_VERSION_COMPUTE,
                "name": vm_name,
                "location": client.settings.location,
                "dependsOn": [
                    format!("[resourceId('Microsoft.Network/networkInterfaces', '{}-nic')]", vm_name)
                ],
                "properties": {
                    "hardwareProfile": {
                        "vmSize": client.settings.vm_size
                    },
                    "osProfile": {
                        "computerName": vm_name,
                        "adminUsername": client.settings.admin_username,
                        "linuxConfiguration": {
                            "disablePasswordAuthentication": true,
                            "ssh": {
                                "publicKeys": [
                                    {
                                        "path": format!("/home/{}/.ssh/authorized_keys", client.settings.admin_username),
                                        "keyData": ssh_public_key
                                    }
                                ]
                            }
                        }
                    },
                    "storageProfile": {
                        "imageReference": {
                            "publisher": publisher,
                            "offer": offer,
                            "sku": sku,
                            "version": "latest"
                        },
                        "osDisk": {
                            "name": format!("{}-osdisk", vm_name),
                            "caching": "ReadWrite",
                            "createOption": "FromImage"
                        }
                    },
                    "networkProfile": {
                        "networkInterfaces": [
                            {
                                "id": format!("[resourceId('Microsoft.Network/networkInterfaces', '{}-nic')]", vm_name)
                            }
                        ]
                    }
                }
            }
        ]
    });

    let deployment_name = format!("herdr-deploy-{}", vm_name);
    let path = format!(
        "/subscriptions/{}/resourcegroups/{}/providers/Microsoft.Resources/deployments/{}?api-version={}",
        client.settings.subscription_id,
        client.settings.resource_group,
        deployment_name,
        API_VERSION_RESOURCES
    );

    let body = serde_json::json!({
        "properties": {
            "mode": "Incremental",
            "template": template
        }
    });

    let _: serde_json::Value = client.put(&path, &body)?;
    Ok(())
}

pub(crate) fn get_vm_power_state(client: &Client, vm_name: &str) -> Result<PowerState> {
    let path = format!(
        "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Compute/virtualMachines/{}/instanceView?api-version={}",
        client.settings.subscription_id,
        client.settings.resource_group,
        vm_name,
        API_VERSION_COMPUTE
    );
    let view: InstanceView = client.get(&path)?;
    for item in view.statuses {
        match item.code.as_str() {
            "PowerState/running" => return Ok(PowerState::Running),
            "PowerState/stopped" => return Ok(PowerState::Stopped),
            "PowerState/deallocated" => return Ok(PowerState::Deallocated),
            "PowerState/starting" => return Ok(PowerState::Starting),
            _ => {}
        }
    }
    Ok(PowerState::Unknown)
}

pub(crate) fn get_vm_public_ip(client: &Client, vm_name: &str) -> Result<Option<String>> {
    let ip_name = format!("{}-ip", vm_name);
    let path = format!(
        "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Network/publicIPAddresses/{}?api-version={}",
        client.settings.subscription_id,
        client.settings.resource_group,
        ip_name,
        API_VERSION_NETWORK
    );
    let resp: PublicIpResponse = client.get(&path)?;
    Ok(resp.properties.and_then(|p| p.ip_address))
}

pub(crate) fn start_vm(client: &Client, vm_name: &str) -> Result<()> {
    let path = format!(
        "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Compute/virtualMachines/{}/start?api-version={}",
        client.settings.subscription_id,
        client.settings.resource_group,
        vm_name,
        API_VERSION_COMPUTE
    );
    let _: serde_json::Value = client.post_empty(&path)?;
    Ok(())
}

#[allow(dead_code)]
pub(crate) fn deallocate_vm(client: &Client, vm_name: &str) -> Result<()> {
    let path = format!(
        "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Compute/virtualMachines/{}/deallocate?api-version={}",
        client.settings.subscription_id,
        client.settings.resource_group,
        vm_name,
        API_VERSION_COMPUTE
    );
    let _: serde_json::Value = client.post_empty(&path)?;
    Ok(())
}

#[allow(dead_code)]
pub(crate) fn delete_vm(client: &Client, vm_name: &str) -> Result<()> {
    let path = format!(
        "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Compute/virtualMachines/{}?api-version={}",
        client.settings.subscription_id,
        client.settings.resource_group,
        vm_name,
        API_VERSION_COMPUTE
    );
    let _: serde_json::Value = client.delete(&path)?;
    Ok(())
}

#[allow(dead_code)]
pub(crate) fn delete_vm_and_resources(client: &Client, vm_name: &str) -> Result<()> {
    let sub = &client.settings.subscription_id;
    let rg = &client.settings.resource_group;

    let _ = delete_vm(client, vm_name);

    let schedule_path = format!(
        "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.DevTestLab/schedules/shutdown-computevm-{}?api-version=2018-09-15",
        sub, rg, vm_name
    );
    let _: std::result::Result<serde_json::Value, _> = client.delete(&schedule_path);

    let nic_path = format!(
        "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Network/networkInterfaces/{}-nic?api-version={}",
        sub, rg, vm_name, API_VERSION_NETWORK
    );
    let _: std::result::Result<serde_json::Value, _> = client.delete(&nic_path);

    let disk_path = format!(
        "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Compute/disks/{}-osdisk?api-version={}",
        sub, rg, vm_name, API_VERSION_COMPUTE
    );
    let _: std::result::Result<serde_json::Value, _> = client.delete(&disk_path);

    let ip_path = format!(
        "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Network/publicIPAddresses/{}-ip?api-version={}",
        sub, rg, vm_name, API_VERSION_NETWORK
    );
    let _: std::result::Result<serde_json::Value, _> = client.delete(&ip_path);

    let nsg_path = format!(
        "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Network/networkSecurityGroups/{}-nsg?api-version={}",
        sub, rg, vm_name, API_VERSION_NETWORK
    );
    let _: std::result::Result<serde_json::Value, _> = client.delete(&nsg_path);

    let vnet_path = format!(
        "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Network/virtualNetworks/{}-vnet?api-version={}",
        sub, rg, vm_name, API_VERSION_NETWORK
    );
    let _: std::result::Result<serde_json::Value, _> = client.delete(&vnet_path);

    Ok(())
}

/// The size of each VM in the resource group, by lowercase name.
pub(crate) fn vm_sizes(client: &Client) -> Result<std::collections::HashMap<String, String>> {
    let path = format!(
        "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Compute/virtualMachines?api-version={}",
        client.settings.subscription_id, client.settings.resource_group, API_VERSION_COMPUTE
    );
    let list: VmList = client.get(&path)?;
    Ok(list
        .value
        .into_iter()
        .filter_map(|vm| {
            let size = vm.properties?.hardware?.size?;
            Some((vm.name.to_ascii_lowercase(), size))
        })
        .collect())
}

pub(crate) fn wait_running_ip(
    client: &Client,
    vm_name: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<String> {
    let deadline = Instant::now() + TIMEOUT;
    while Instant::now() < deadline {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        if let Ok(PowerState::Running) = get_vm_power_state(client, vm_name)
            && let Ok(Some(ip)) = get_vm_public_ip(client, vm_name)
            && !ip.is_empty()
        {
            return Ok(ip);
        }
        std::thread::sleep(POLL_INTERVAL);
    }
    Err(Error::StartTimeout)
}
