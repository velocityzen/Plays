use std::{
    fmt::Debug,
    hash::{Hash, Hasher},
};

use anyhow::Context as _;
use uuid::Uuid;
use wayland_backend::client::ObjectId;

use crate::{Bounds, DisplayId, Pixels, PlatformDisplay};

#[derive(Debug, Clone)]
pub(crate) struct WaylandDisplay {
    /// The ID of the wl_output object
    pub id: ObjectId,
    pub name: Option<String>,
    /// Plays.
    pub description: Option<String>,
    pub bounds: Bounds<Pixels>,
}

impl Hash for WaylandDisplay {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl PlatformDisplay for WaylandDisplay {
    fn id(&self) -> DisplayId {
        DisplayId(self.id.protocol_id())
    }

    fn uuid(&self) -> anyhow::Result<Uuid> {
        let name = self
            .name
            .as_ref()
            .context("Wayland display does not have a name")?;
        Ok(Uuid::new_v5(&Uuid::NAMESPACE_DNS, name.as_bytes()))
    }

    fn bounds(&self) -> Bounds<Pixels> {
        self.bounds
    }

    // Plays: as the compositor describes it, with its connector's name,
    // which tells two of the same model apart, unless it says it already.
    fn name(&self) -> Option<String> {
        match (&self.description, &self.name) {
            (Some(description), Some(name)) if !description.contains(name.as_str()) => {
                Some(format!("{description} ({name})"))
            }
            (Some(description), _) => Some(description.clone()),
            (None, name) => name.clone(),
        }
    }
}
