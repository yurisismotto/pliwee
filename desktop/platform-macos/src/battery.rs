//! This Mac's own battery, for `battery.v1`.
//!
//! The macOS counterpart of `pliwee-capability-battery`'s UPower reader, with
//! the same presence rule: a Mac with a battery reports it, a Mac without one
//! (a Mac mini, a Mac Studio) is receive-only, and a Mac whose power sources
//! cannot be read says nothing rather than a fabricated 0%.
//!
//! # Why FFI
//!
//! IOKit's power-source API — `IOPSCopyPowerSourcesInfo`,
//! `IOPSCopyPowerSourcesList`, `IOPSGetPowerSourceDescription` — is the
//! documented way to read the internal battery, and it is a C API with no
//! safe Rust binding in this workspace. It is three functions. The `unsafe`
//! is confined to [`snapshot`], which copies what it needs out of Core
//! Foundation objects into a plain [`PowerSource`] before returning; the
//! mapping to the wire, [`reading_of`], is safe and is tested without IOKit.
//!
//! No other `unsafe` is needed: ownership of the Core Foundation objects is
//! taken by `core-foundation`'s wrappers, which release them on drop.

use std::sync::Arc;

use core_foundation::array::{CFArray, CFArrayRef};
use core_foundation::base::{CFType, CFTypeRef, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;
use pliwee_capability_battery::{BatteryReading, LocalBatterySource};
use pliwee_proto::v1::capabilities::ChargingState;

// `IOKit/ps/IOPowerSources.h`. The returned objects follow the Core
// Foundation naming rule: `Copy` transfers ownership to the caller, `Get`
// does not.
#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOPSCopyPowerSourcesInfo() -> CFTypeRef;
    fn IOPSCopyPowerSourcesList(blob: CFTypeRef) -> CFArrayRef;
    fn IOPSGetPowerSourceDescription(blob: CFTypeRef, ps: CFTypeRef) -> CFDictionaryRef;
}

// `IOKit/ps/IOPSKeys.h`. The values of the `kIOPS…Key` macros, which are
// string literals in the header rather than exported symbols.
const KEY_TYPE: &str = "Type";
const KEY_IS_PRESENT: &str = "Is Present";
const KEY_CURRENT_CAPACITY: &str = "Current Capacity";
const KEY_MAX_CAPACITY: &str = "Max Capacity";
const KEY_IS_CHARGING: &str = "Is Charging";
const KEY_IS_CHARGED: &str = "Is Charged";
const KEY_POWER_SOURCE_STATE: &str = "Power Source State";

const TYPE_INTERNAL_BATTERY: &str = "InternalBattery";
const STATE_AC: &str = "AC Power";
const STATE_BATTERY: &str = "Battery Power";

/// What one power source said, copied out of its Core Foundation dictionary.
///
/// Every field is optional because IOKit's own documentation marks most keys
/// optional, and a UPS or an external battery describes itself with a
/// different subset from the internal one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PowerSource {
    pub kind: Option<String>,
    pub present: Option<bool>,
    pub current_capacity: Option<i64>,
    pub max_capacity: Option<i64>,
    pub charging: Option<bool>,
    pub charged: Option<bool>,
    pub state: Option<String>,
}

impl PowerSource {
    fn is_internal_battery(&self) -> bool {
        self.kind.as_deref() == Some(TYPE_INTERNAL_BATTERY) && self.present != Some(false)
    }
}

/// Whether this Mac has a battery to report.
pub enum MacBattery {
    /// An internal battery is present; report it to peers.
    Present(IopsBattery),
    /// The power-source API answered and lists no internal battery.
    Absent,
    /// The power-source API returned nothing at all.
    Unavailable,
}

impl MacBattery {
    /// Probes once, at startup.
    pub fn detect() -> Self {
        match snapshot() {
            None => Self::Unavailable,
            Some(sources) if sources.iter().any(PowerSource::is_internal_battery) => {
                Self::Present(IopsBattery)
            }
            Some(_) => Self::Absent,
        }
    }
}

/// The internal battery, read afresh on every [`read`](LocalBatterySource::read).
#[derive(Debug, Clone, Copy)]
pub struct IopsBattery;

impl IopsBattery {
    pub fn into_source(self) -> Arc<dyn LocalBatterySource> {
        Arc::new(self)
    }
}

#[async_trait::async_trait]
impl LocalBatterySource for IopsBattery {
    async fn read(&self) -> Option<BatteryReading> {
        // IOKit's power-source calls are synchronous. They are quick, but a
        // blocking call has no business on an async worker regardless.
        let sources = tokio::task::spawn_blocking(snapshot).await.ok()??;
        reading_of(&sources, now_ms())
    }
}

/// Turns a snapshot into a reading, or `None` when there is nothing truthful
/// to say.
///
/// `None` means "send nothing", never "send zero" — the rule the UPower
/// reader follows, for the same reason.
pub fn reading_of(sources: &[PowerSource], now_unix_ms: i64) -> Option<BatteryReading> {
    let battery = sources.iter().find(|s| s.is_internal_battery())?;
    let current = battery.current_capacity?;
    let max = battery.max_capacity?;
    // A zero or negative maximum is a half-initialised reading, not an empty
    // battery.
    if max <= 0 || current < 0 {
        return None;
    }
    let percentage = ((current as f64 * 100.0 / max as f64).round()).clamp(0.0, 100.0) as u32;
    Some(BatteryReading {
        percentage,
        charging_state: charging_state(battery, percentage),
        peer_timestamp_unix_ms: now_unix_ms,
    })
}

/// IOKit's three facts -> the wire's one enum.
///
/// "On AC and not charging" is the steady state of a Mac with Optimized
/// Battery Charging holding the pack at 80%, which is why it maps to
/// `NOT_CHARGING` rather than to `FULL` unless the pack says it is charged.
fn charging_state(s: &PowerSource, percentage: u32) -> ChargingState {
    if s.charging == Some(true) {
        return ChargingState::Charging;
    }
    match s.state.as_deref() {
        Some(STATE_AC) if s.charged == Some(true) || percentage >= 100 => ChargingState::Full,
        Some(STATE_AC) => ChargingState::NotCharging,
        Some(STATE_BATTERY) => ChargingState::Discharging,
        _ => ChargingState::Unspecified,
    }
}

/// Every power source this Mac reports, or `None` when IOKit returns nothing.
#[allow(unsafe_code)]
fn snapshot() -> Option<Vec<PowerSource>> {
    // SAFETY: `IOPSCopyPowerSourcesInfo` takes no arguments and returns an
    // owned (+1) CFTypeRef or NULL. NULL is checked before the wrapper is
    // built; `wrap_under_create_rule` takes over the +1 and releases it when
    // `blob` drops, at the end of this function — after every use below.
    let blob = unsafe {
        let raw = IOPSCopyPowerSourcesInfo();
        if raw.is_null() {
            return None;
        }
        CFType::wrap_under_create_rule(raw)
    };

    // SAFETY: `blob` is a live power-sources blob for the duration of this
    // call. The function returns an owned (+1) CFArrayRef or NULL; NULL is
    // checked, and `wrap_under_create_rule` takes over the +1. The array's
    // elements are CFTypeRefs, which is what `CFArray<CFType>` declares.
    let list: CFArray<CFType> = unsafe {
        let raw = IOPSCopyPowerSourcesList(blob.as_CFTypeRef());
        if raw.is_null() {
            return None;
        }
        CFArray::wrap_under_create_rule(raw)
    };

    let mut sources = Vec::with_capacity(list.len() as usize);
    for ps in list.iter() {
        // SAFETY: both arguments are live for the call: `blob` is held above
        // and `ps` borrows from `list`. The return value is a non-owned (+0)
        // CFDictionaryRef or NULL, valid while `blob` lives. NULL is checked;
        // `wrap_under_get_rule` retains it, so the wrapper owns its own
        // reference and does not depend on `blob` afterwards. The keys of a
        // power-source description are CFStrings (IOPSKeys.h).
        let description: CFDictionary<CFString, CFType> = unsafe {
            let raw = IOPSGetPowerSourceDescription(blob.as_CFTypeRef(), ps.as_CFTypeRef());
            if raw.is_null() {
                continue;
            }
            CFDictionary::wrap_under_get_rule(raw)
        };
        sources.push(describe(&description));
    }
    Some(sources)
}

/// Copies the keys this module reads out of one description. Safe.
fn describe(d: &CFDictionary<CFString, CFType>) -> PowerSource {
    let get = |key: &str| d.find(CFString::new(key)).map(|v| v.clone());
    let string = |key: &str| {
        get(key)
            .and_then(|v| v.downcast::<CFString>())
            .map(|s| s.to_string())
    };
    let number = |key: &str| {
        get(key)
            .and_then(|v| v.downcast::<CFNumber>())
            .and_then(|n| n.to_i64())
    };
    let boolean = |key: &str| {
        get(key)
            .and_then(|v| v.downcast::<CFBoolean>())
            .map(bool::from)
    };
    PowerSource {
        kind: string(KEY_TYPE),
        present: boolean(KEY_IS_PRESENT),
        current_capacity: number(KEY_CURRENT_CAPACITY),
        max_capacity: number(KEY_MAX_CAPACITY),
        charging: boolean(KEY_IS_CHARGING),
        charged: boolean(KEY_IS_CHARGED),
        state: string(KEY_POWER_SOURCE_STATE),
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn internal(current: i64, max: i64, state: &str, charging: bool) -> PowerSource {
        PowerSource {
            kind: Some(TYPE_INTERNAL_BATTERY.into()),
            present: Some(true),
            current_capacity: Some(current),
            max_capacity: Some(max),
            charging: Some(charging),
            charged: None,
            state: Some(state.into()),
        }
    }

    #[test]
    fn a_discharging_battery_reports_its_level() {
        let r = reading_of(&[internal(57, 100, STATE_BATTERY, false)], 7).expect("reading");
        assert_eq!(r.percentage, 57);
        assert_eq!(r.charging_state, ChargingState::Discharging);
        assert_eq!(r.peer_timestamp_unix_ms, 7);
    }

    #[test]
    fn charging_wins_over_the_power_source_state() {
        let r = reading_of(&[internal(40, 100, STATE_AC, true)], 0).expect("reading");
        assert_eq!(r.charging_state, ChargingState::Charging);
    }

    #[test]
    fn plugged_in_and_held_below_full_is_not_charging_rather_than_full() {
        // Optimized Battery Charging parks the pack at 80% on AC.
        let r = reading_of(&[internal(80, 100, STATE_AC, false)], 0).expect("reading");
        assert_eq!(r.charging_state, ChargingState::NotCharging);
    }

    #[test]
    fn plugged_in_and_charged_is_full() {
        let mut s = internal(100, 100, STATE_AC, false);
        assert_eq!(
            reading_of(std::slice::from_ref(&s), 0).map(|r| r.charging_state),
            Some(ChargingState::Full)
        );
        s.current_capacity = Some(97);
        s.charged = Some(true);
        assert_eq!(
            reading_of(&[s], 0).map(|r| r.charging_state),
            Some(ChargingState::Full)
        );
    }

    #[test]
    fn capacity_is_a_ratio_not_assumed_to_be_out_of_one_hundred() {
        let r = reading_of(&[internal(2500, 5000, STATE_BATTERY, false)], 0).expect("reading");
        assert_eq!(r.percentage, 50);
        let over = reading_of(&[internal(5100, 5000, STATE_BATTERY, false)], 0).expect("reading");
        assert_eq!(over.percentage, 100, "clamped, as the UPower reader does");
    }

    #[test]
    fn a_mac_without_a_battery_says_nothing_rather_than_zero() {
        let ups = PowerSource {
            kind: Some("UPS".into()),
            present: Some(true),
            current_capacity: Some(90),
            max_capacity: Some(100),
            ..PowerSource::default()
        };
        assert_eq!(reading_of(&[ups], 0), None);
        assert_eq!(reading_of(&[], 0), None);
    }

    #[test]
    fn a_half_initialised_battery_says_nothing() {
        let mut s = internal(50, 0, STATE_BATTERY, false);
        assert_eq!(reading_of(std::slice::from_ref(&s), 0), None);
        s.max_capacity = None;
        assert_eq!(reading_of(std::slice::from_ref(&s), 0), None);
        s.max_capacity = Some(100);
        s.current_capacity = None;
        assert_eq!(reading_of(&[s], 0), None);
    }

    #[test]
    fn a_battery_marked_not_present_is_not_reported() {
        let mut s = internal(50, 100, STATE_BATTERY, false);
        s.present = Some(false);
        assert_eq!(reading_of(&[s], 0), None);
    }

    #[test]
    fn the_real_power_source_api_answers_on_this_mac() {
        // Measured, not assumed: IOKit returns a snapshot on any Mac. Whether
        // it lists a battery depends on the hardware, so that part is
        // reported rather than asserted — but a battery that is listed must
        // produce a reading, or the mapping above is wrong for real data.
        let sources = snapshot().expect("IOPSCopyPowerSourcesInfo returned NULL");
        if sources.iter().any(PowerSource::is_internal_battery) {
            let r = reading_of(&sources, now_ms()).expect("an internal battery must read");
            assert!(r.percentage <= 100);
            assert_ne!(r.charging_state, ChargingState::Unspecified, "{sources:?}");
        } else {
            eprintln!("no internal battery on this Mac: {sources:?}");
        }
    }
}
