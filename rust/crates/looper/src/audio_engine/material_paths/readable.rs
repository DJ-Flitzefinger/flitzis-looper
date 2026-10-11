//! Durable locators for readable roots. A binding never replaces full source/PCM verification.
use super::*;
use serde::{Deserialize, Serialize};
use std::io::Read;

pub(super) const BINDING_LEAF: &str = ".material.json";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::audio_engine) struct MaterialBinding {
    pub schema_version: u64,
    pub sha256: String,
    pub bytes: u64,
    pub original_leaf: String,
}

impl MaterialBinding {
    pub(in crate::audio_engine) fn material_id(&self) -> String {
        self.sha256[..32].to_owned()
    }

    pub(in crate::audio_engine) fn validate(&self) -> io::Result<()> {
        if self.schema_version != 1
            || self.sha256.len() != 64
            || !self
                .sha256
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
            || !filename(&self.original_leaf)
            || [".material.json", "stems", ".pcm-cache"]
                .iter()
                .any(|reserved| self.original_leaf.eq_ignore_ascii_case(reserved))
        {
            return Err(invalid("invalid readable material binding"));
        }
        Ok(())
    }
}

pub(in crate::audio_engine) fn read_binding(root: &Path) -> io::Result<MaterialBinding> {
    read_binding_held(root).map(|(binding, _reader)| binding)
}

pub(in crate::audio_engine) fn read_binding_held(
    root: &Path,
) -> io::Result<(MaterialBinding, std::fs::File)> {
    let _guards = super::super::project_assets::directory_guards(root)?;
    super::super::project_assets::reject_links(&root.join(BINDING_LEAF))?;
    let mut reader = super::super::cold_store::sealed_reader(&root.join(BINDING_LEAF))?;
    let size = reader.metadata()?.len();
    if size == 0 || size > 8192 {
        return Err(invalid("readable binding exceeds metadata bound"));
    }
    let mut bytes = vec![0; size as usize];
    reader.read_exact(&mut bytes)?;
    if reader.read(&mut [0; 1])? != 0 {
        return Err(invalid("readable binding EOF mismatch"));
    }
    let binding: MaterialBinding = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
    binding.validate()?;
    Ok((binding, reader))
}

pub(super) fn readable_container(name: &str) -> bool {
    filename(name)
        && !name.starts_with(['.', '#'])
        && !["samples", "materials", "stems", "config.json"]
            .iter()
            .any(|reserved| name.eq_ignore_ascii_case(reserved))
}

/// Windows names are disambiguated for the directory only; the supported source leaf stays exact.
pub(in crate::audio_engine) fn readable_name(leaf: &str) -> String {
    let mut name = String::new();
    let mut units = 0;
    for character in leaf.chars() {
        if units + character.len_utf16() > 96 {
            break;
        }
        name.push(
            if character.is_control() || "<>:\"/\\|?*".contains(character) {
                '_'
            } else {
                character
            },
        );
        units += character.len_utf16();
    }
    let name = name.trim_end_matches(['.', ' ']).to_owned();
    if readable_container(&name) {
        name
    } else {
        format!("Source_{}", name.trim_start_matches(['.', '#']))
    }
}
