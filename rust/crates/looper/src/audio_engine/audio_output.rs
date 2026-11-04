//! Requested output intent and control-thread discovery. No device runs here.

use cpal::traits::{DeviceTrait, HostTrait};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct OutputOptions {
    pub backend: String,
    pub device_id: Option<String>,
    pub sample_rate_hz: Option<u32>,
    pub buffer_frames: Option<u32>,
    pub left_channel: Option<u32>,
    pub right_channel: Option<u32>,
    // Internal captured rollback policy, separate from Human Auto preference.
    #[cfg(windows)]
    #[serde(skip)]
    pub wasapi_match_engine_format: Option<bool>,
    #[cfg(all(windows, test, feature = "asio-contract-tests"))]
    #[serde(skip)]
    pub test_driver: Option<std::sync::Arc<super::asio_output::test_driver::Factory>>,
}

impl Default for OutputOptions {
    fn default() -> Self {
        Self {
            backend: "auto".into(),
            device_id: None,
            sample_rate_hz: None,
            buffer_frames: None,
            left_channel: None,
            right_channel: None,
            #[cfg(windows)]
            wasapi_match_engine_format: None,
            #[cfg(all(windows, test, feature = "asio-contract-tests"))]
            test_driver: None,
        }
    }
}

impl OutputOptions {
    pub fn parse(json: Option<&str>) -> Result<Self, String> {
        let options: Self = match json {
            Some(value) => serde_json::from_str(value).map_err(|error| error.to_string())?,
            None => Self::default(),
        };
        if options.backend.is_empty()
            || options.backend.len() > 32
            || !options.backend.as_bytes()[0].is_ascii_lowercase()
            || !options.backend.bytes().all(|value| {
                value.is_ascii_lowercase()
                    || value.is_ascii_digit()
                    || value == b'_'
                    || value == b'-'
            })
        {
            return Err("Invalid output backend identifier".into());
        }
        if options
            .device_id
            .as_ref()
            .is_some_and(|id| id.is_empty() || id.len() > 4096)
            || options.sample_rate_hz == Some(0)
            || options
                .buffer_frames
                .is_some_and(|frames| frames == 0 || frames > i32::MAX as u32)
            || options
                .left_channel
                .is_some_and(|channel| channel > i32::MAX as u32)
            || options
                .right_channel
                .is_some_and(|channel| channel > i32::MAX as u32)
        {
            return Err("Invalid output device, rate, buffer or channel".into());
        }
        match (options.left_channel, options.right_channel) {
            (None, None) => {}
            (Some(left), Some(right)) if left != right => {}
            _ => return Err("Choose two distinct physical outputs or automatic routing".into()),
        }
        Ok(options)
    }

    pub fn physical_pair(&self, channels: usize) -> Result<(usize, usize), String> {
        let left = self.left_channel.unwrap_or(0) as usize;
        let right = self
            .right_channel
            .unwrap_or(if channels == 1 { 0 } else { 1 }) as usize;
        if channels == 0 || left >= channels || right >= channels {
            return Err("Selected physical output is unavailable".into());
        }
        Ok((left, right))
    }

    pub fn cpal_device(&self) -> Result<(cpal::Host, cpal::Device), String> {
        let host = if self.backend == "auto" {
            cpal::default_host()
        } else {
            let id = cpal::available_hosts()
                .into_iter()
                .find(|id| id.name().to_ascii_lowercase() == self.backend)
                .ok_or_else(|| format!("Output backend '{}' is unavailable", self.backend))?;
            cpal::host_from_id(id).map_err(|error| error.to_string())?
        };
        let device = if let Some(selected) = &self.device_id {
            host.output_devices()
                .map_err(|error| error.to_string())?
                .find(|device| device.id().is_ok_and(|id| id.to_string() == *selected))
                .ok_or_else(|| "Selected output device is unavailable".to_owned())?
        } else {
            host.default_output_device()
                .ok_or_else(|| "No default output device found".to_owned())?
        };
        Ok((host, device))
    }
}

#[derive(Serialize)]
pub(super) struct OutputEndpoint {
    pub id: String,
    pub name: String,
}

#[derive(Serialize)]
pub(super) struct OutputBackend {
    pub id: String,
    pub name: String,
    pub devices: Vec<OutputEndpoint>,
    pub discovery_error: Option<String>,
}

pub(super) fn cpal_inventory() -> Vec<OutputBackend> {
    cpal::available_hosts()
        .into_iter()
        .map(|id| {
            let mut backend = OutputBackend {
                id: id.name().to_ascii_lowercase(),
                name: id.name().to_owned(),
                devices: Vec::new(),
                discovery_error: None,
            };
            match cpal::host_from_id(id)
                .map_err(|error| error.to_string())
                .and_then(|host| host.output_devices().map_err(|error| error.to_string()))
            {
                Ok(devices) => {
                    for device in devices {
                        match device
                            .id()
                            .map_err(|error| error.to_string())
                            .and_then(|id| {
                                device
                                    .description()
                                    .map(|description| OutputEndpoint {
                                        id: id.to_string(),
                                        name: description.name().to_owned(),
                                    })
                                    .map_err(|error| error.to_string())
                            }) {
                            Ok(endpoint) => backend.devices.push(endpoint),
                            Err(error) => backend.discovery_error = Some(error),
                        }
                    }
                }
                Err(error) => backend.discovery_error = Some(error),
            }
            backend
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_output_intent_rejects_coercion_and_partial_routing() {
        for json in [
            r#"{"buffer_frames":true}"#,
            r#"{"sample_rate_hz":0}"#,
            r#"{"backend":"ASIO"}"#,
            r#"{"left_channel":0}"#,
            r#"{"left_channel":2,"right_channel":2}"#,
            r#"{"unknown":0}"#,
        ] {
            assert!(OutputOptions::parse(Some(json)).is_err(), "{json}");
        }
        assert_eq!(OutputOptions::default().physical_pair(1).unwrap(), (0, 0));
        let options = OutputOptions::parse(Some(
            r#"{"backend":"asio","left_channel":4,"right_channel":5,"buffer_frames":96}"#,
        ))
        .unwrap();
        assert_eq!(options.physical_pair(12).unwrap(), (4, 5));
        assert!(options.physical_pair(4).is_err());
    }
}
