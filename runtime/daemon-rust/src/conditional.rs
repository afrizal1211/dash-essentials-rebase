//! Conditional profiles, the stack above the global profile.
//!
//! A per-app profile is the strongest statement a user can make: this app,
//! these controls. Below it sits the global profile, which is everything
//! else. Between the two, a conditional profile answers a state of the
//! device rather than an app: what the GPU should do while charging, what
//! the CPU should do while the screen is off.
//!
//! The stack has one rule, and it is fixed so the user never has to wonder
//! which condition wins. From strongest to weakest: a charging condition, a
//! screen-off condition, a screen-on condition, and then the global profile.
//! Charging a device changes its thermal budget outright, so it outranks
//! everything below the per-app layer; the screen state follows because it
//! is the next coarsest fact about the device. A conditional never overrides
//! a foreground app's own profile, which keeps the layer the user chose for
//! an app authoritative while that app is in front.

use super::app_controls::{Capabilities, Profile};
use serde_json::{Value, json};
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

const SCHEMA: u64 = 1;
const MAX_CONDITIONALS: usize = 32;
const MAX_NAME: usize = 32;

/// One conditional profile: a name, the state it answers, and the controls.
#[derive(Clone, Debug)]
pub(super) struct Conditional {
    name: String,
    enabled: bool,
    /// `Some(true)` means the device must be charging, `Some(false)` that it
    /// must not be. `None` does not care.
    charging: Option<bool>,
    /// `Some(true)` means the screen must be on, `Some(false)` that it must
    /// be off. `None` does not care.
    screen_on: Option<bool>,
    profile: Profile,
}

/// The whole stack, persisted as one file next to the per-app profiles.
#[derive(Clone, Debug, Default)]
pub(super) struct Stack {
    conditionals: Vec<Conditional>,
}

impl Conditional {
    fn parse(value: &Value) -> Result<Self, String> {
        let name = value["name"].as_str().unwrap_or_default().trim();
        if name.is_empty() || name.len() > MAX_NAME {
            return Err("conditional name must be 1..=32 characters".into());
        }
        if !name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
        {
            return Err("conditional name may only contain letters, digits, _ and -".into());
        }
        let charging = match value.get("charging") {
            Some(Value::Bool(v)) => Some(*v),
            Some(Value::Null) | None => None,
            _ => return Err("conditional charging must be true, false or null".into()),
        };
        let screen_on = match value.get("screen_on") {
            Some(Value::Bool(v)) => Some(*v),
            Some(Value::Null) | None => None,
            _ => return Err("conditional screen_on must be true, false or null".into()),
        };
        let profile = Profile::parse(&value["profile"])?;
        Ok(Self {
            name: name.to_owned(),
            enabled: value["enabled"].as_bool().unwrap_or(true),
            charging,
            screen_on,
            profile,
        })
    }

    /// The controls this conditional applies. Used by the runtime when it
    /// resolves the stack, so the profile can be handed to the adapter.
    pub(super) fn profile(&self) -> &Profile {
        &self.profile
    }

    pub(super) fn json(&self) -> Value {
        json!({
            "name": self.name,
            "enabled": self.enabled,
            "charging": self.charging,
            "screen_on": self.screen_on,
            "profile": self.profile.json(),
        })
    }

    fn matches(&self, charging: bool, screen_on: bool) -> bool {
        self.charging.is_none_or(|wanted| wanted == charging)
            && self.screen_on.is_none_or(|wanted| wanted == screen_on)
    }

    /// The precedence of the condition this profile answers. Lower is
    /// stronger. Charging comes first because it changes the thermal budget
    /// of the whole device, then the screen state, and a conditional that
    /// names no condition is the weakest of all because it is always true.
    fn rank(&self) -> u8 {
        match (self.charging.is_some(), self.screen_on) {
            (true, _) => 0,
            (false, Some(false)) => 1,
            (false, Some(true)) => 2,
            (false, None) => 3,
        }
    }
}

impl Stack {
    fn path() -> PathBuf {
        crate::state_dir().join("conditionals.json")
    }

    pub(super) fn load() -> Self {
        let Ok(raw) = fs::read_to_string(Self::path()) else {
            return Stack::default();
        };
        // A damaged file costs the user their stack, so a bad entry is
        // skipped instead of dropping everything; the good entries survive.
        let Ok(value) = serde_json::from_str::<Value>(&raw) else {
            return Stack::default();
        };
        let Some(items) = value["conditionals"].as_array() else {
            return Stack::default();
        };
        let conditionals = items
            .iter()
            .filter_map(|item| Conditional::parse(item).ok())
            .take(MAX_CONDITIONALS)
            .collect();
        Stack { conditionals }
    }

    pub(super) fn save(&self) -> Result<(), String> {
        let raw = json!({
            "schema": SCHEMA,
            "conditionals": self.conditionals.iter().map(Conditional::json).collect::<Vec<_>>(),
        });
        let dir = crate::state_dir();
        if !dir.exists() {
            return Err("state directory missing".into());
        }
        let text = serde_json::to_string(&raw).map_err(|e| e.to_string())?;
        // Write to a sibling temp file, then rename, so a fault never leaves
        // a truncated stack in place of a good one.
        let tmp = dir.join("conditionals.json.tmp");
        let mut file = fs::File::create(&tmp).map_err(|e| e.to_string())?;
        file.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        drop(file);
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600)).map_err(|e| e.to_string())?;
        fs::rename(&tmp, Self::path()).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// The strongest enabled conditional that the device currently matches,
    /// or none, in which case the global profile applies as before.
    pub(super) fn active(&self, charging: bool, screen_on: bool) -> Option<&Conditional> {
        let mut best: Option<&Conditional> = None;
        for conditional in &self.conditionals {
            if !conditional.enabled || !conditional.matches(charging, screen_on) {
                continue;
            }
            if best.is_none_or(|other| conditional.rank() < other.rank()) {
                best = Some(conditional);
            }
        }
        best
    }

    pub(super) fn list(&self) -> Value {
        json!({
            "schema": SCHEMA,
            "conditionals": self.conditionals.iter().map(Conditional::json).collect::<Vec<_>>(),
            "charging": crate::usb("online") == "1",
            "screenOn": crate::screen_is_on() == Some(true),
        })
    }

    /// Add or replace one conditional by name. The profile is validated
    /// against the live capabilities, exactly the way a per-app profile is,
    /// so the stack cannot store a control the device cannot take.
    pub(super) fn upsert(&mut self, value: &Value, caps: &Capabilities) -> Result<(), String> {
        let conditional = Conditional::parse(value)?;
        caps.validate(&conditional.profile)?;
        if !conditional.enabled {
            // A stored but disabled conditional is still validated, because it
            // is one tap away from being enabled.
        }
        if self.conditionals.iter().any(|c| c.name == conditional.name) {
            self.conditionals.retain(|c| c.name != conditional.name);
        } else if self.conditionals.len() >= MAX_CONDITIONALS {
            return Err("too many conditionals".into());
        }
        self.conditionals.push(conditional);
        self.conditionals.sort_by_key(|c| c.rank());
        Ok(())
    }

    pub(super) fn remove(&mut self, name: &str) -> Result<(), String> {
        let before = self.conditionals.len();
        self.conditionals.retain(|c| c.name != name);
        if self.conditionals.len() == before {
            return Err("unknown conditional name".into());
        }
        Ok(())
    }

    /// Replace the whole stack from imported JSON, validating every entry
    /// against the live capabilities the way `upsert` validates one. Entries
    /// this device cannot honour are dropped and named in the answer, because
    /// an import must never store a control this kernel cannot take; an entry
    /// that does not even parse refuses the replace outright, so a malformed
    /// bundle cannot erase a good stack. Replaces, never merges: an import
    /// replaces everything.
    pub(super) fn replace(
        &mut self,
        items: &Value,
        caps: &Capabilities,
    ) -> Result<Vec<String>, String> {
        let entries = items.as_array().ok_or("missing conditionals array")?;
        if entries.len() > MAX_CONDITIONALS {
            return Err("too many conditionals".into());
        }
        // Parse and validate everything before touching the stack, so a
        // refusal leaves it exactly as it was.
        let mut replaced = Vec::new();
        let mut rejected = Vec::new();
        for item in entries {
            let conditional = Conditional::parse(item)?;
            match caps.validate(&conditional.profile) {
                Ok(()) => replaced.push(conditional),
                Err(_) => rejected.push(conditional.name),
            }
        }
        replaced.sort_by_key(|conditional| conditional.rank());
        self.conditionals = replaced;
        Ok(rejected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conditional(name: &str, charging: Option<bool>, screen_on: Option<bool>) -> Conditional {
        Conditional {
            name: name.into(),
            enabled: true,
            charging,
            screen_on,
            profile: Profile::default(),
        }
    }

    #[test]
    fn the_stack_prefers_charging_over_the_screen_state() {
        let stack = Stack {
            conditionals: vec![
                conditional("screen-on-job", None, Some(true)),
                conditional("charge-job", Some(true), None),
                conditional("screen-off-saver", None, Some(false)),
            ],
        };
        // Charging with the screen on: the charging condition wins.
        let active = stack.active(true, true).map(|c| c.name.as_str());
        assert_eq!(active, Some("charge-job"));
        // Off the charger with the screen off: the screen-off condition is
        // the strongest one that still matches.
        let active = stack.active(false, false).map(|c| c.name.as_str());
        assert_eq!(active, Some("screen-off-saver"));
        // On battery with the screen on: only the screen-on job matches.
        let active = stack.active(false, true).map(|c| c.name.as_str());
        assert_eq!(active, Some("screen-on-job"));
    }

    #[test]
    fn a_disabled_conditional_is_skipped_and_no_match_falls_back() {
        let mut stack = Stack {
            conditionals: vec![conditional("only", Some(false), None)],
        };
        assert_eq!(
            stack.active(false, true).map(|c| c.name.as_str()),
            Some("only")
        );
        stack.conditionals[0].enabled = false;
        assert_eq!(stack.active(false, true).map(|c| c.name.as_str()), None);
        // A charging-only conditional never matches while charging.
        let plug = Stack {
            conditionals: vec![conditional("plug", Some(true), None)],
        };
        assert_eq!(plug.active(false, false).map(|c| c.name.as_str()), None);
    }

    #[test]
    fn upsert_replaces_by_name_and_sorts_the_stack() {
        let mut stack = Stack::default();
        let caps = Capabilities::default();
        let value = serde_json::json!({
            "name": "charge-job",
            "enabled": true,
            "charging": true,
            "profile": {},
        });
        stack.upsert(&value, &caps).unwrap();
        assert_eq!(stack.conditionals.len(), 1);
        // The same name replaces the entry instead of adding one.
        stack.upsert(&value, &caps).unwrap();
        assert_eq!(stack.conditionals.len(), 1);
        // A bad name is refused.
        let bad = serde_json::json!({ "name": "not legal", "profile": {} });
        assert!(Stack::default().upsert(&bad, &caps).is_err());
        // Removing a name that is not there is refused too.
        assert!(stack.remove("nope").is_err());
        stack.remove("charge-job").unwrap();
        assert!(stack.conditionals.is_empty());
    }

    #[test]
    fn replace_validates_every_entry_and_names_the_refusals() {
        // Default capabilities carry no supported control at all, so a
        // concrete control is always unsupported against them.
        let caps = Capabilities::default();
        let mut stack = Stack::default();
        let items = serde_json::json!([
            {"name": "kept", "enabled": true, "profile": {}},
            {"name": "unsupported", "profile": {"touch": 1}},
        ]);
        let rejected = stack.replace(&items, &caps).unwrap();
        assert_eq!(rejected, vec!["unsupported".to_string()]);
        assert_eq!(
            stack
                .conditionals
                .iter()
                .map(|c| c.name.as_str())
                .collect::<Vec<_>>(),
            vec!["kept"]
        );
        // A malformed entry refuses the replace and leaves the stack intact.
        let bad = serde_json::json!([{"name": "not legal", "profile": {}}]);
        assert!(stack.replace(&bad, &caps).is_err());
        assert_eq!(
            stack
                .conditionals
                .iter()
                .map(|c| c.name.as_str())
                .collect::<Vec<_>>(),
            vec!["kept"]
        );
        // More conditionals than the stack may hold refuses the replace.
        let many: Vec<serde_json::Value> = (0..40)
            .map(|index| serde_json::json!({"name": format!("c{index}"), "profile": {}}))
            .collect();
        assert!(stack.replace(&serde_json::json!(many), &caps).is_err());
    }
}
