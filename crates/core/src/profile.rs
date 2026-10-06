//! Persistent storage for per-device pedal profiles.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Migrations to be applied to the database in order.
const MIGRATIONS: &[&str] = &["CREATE TABLE device_profile (\
        guid TEXT NOT NULL, \
        axis_count INTEGER NOT NULL, \
        button_count INTEGER NOT NULL, \
        profile_json TEXT NOT NULL, \
        updated_at INTEGER NOT NULL, \
        PRIMARY KEY (guid, axis_count, button_count)\
    );"];

/// Unique identifier for an input device.
///
/// SDL GUIDs are not unique, so the key adds the axis and button counts.
/// Two FANATEC Wheel interfaces were seen sharing one GUID with different
/// axis and button counts.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceKey {
    /// SDL GUID string of the device.
    pub guid: String,
    /// Number of axes reported by the device.
    pub axis_count: u32,
    /// Number of buttons reported by the device.
    pub button_count: u32,
}

impl DeviceKey {
    /// Creates a new device key.
    #[must_use]
    pub fn new(guid: impl Into<String>, axis_count: u32, button_count: u32) -> Self {
        Self {
            guid: guid.into(),
            axis_count,
            button_count,
        }
    }
}

/// Pedal type in a pedal set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Pedal {
    /// Throttle (accelerator) pedal.
    Throttle,
    /// Brake pedal.
    Brake,
    /// Clutch pedal.
    Clutch,
}

impl Pedal {
    /// All available pedal variants.
    pub const ALL: [Pedal; 3] = [Pedal::Throttle, Pedal::Brake, Pedal::Clutch];
}

/// Assigned device axis and calibration configuration for a pedal.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PedalAxis {
    /// Physical device axis index.
    pub axis: u8,
    /// Calibration parameters for the axis.
    pub calibration: crate::calibration::AxisCalibration,
}

/// Pedal axes configuration and calibration for a physical device.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceProfile {
    /// Optional throttle pedal axis configuration.
    pub throttle: Option<PedalAxis>,
    /// Optional brake pedal axis configuration.
    pub brake: Option<PedalAxis>,
    /// Optional clutch pedal axis configuration.
    pub clutch: Option<PedalAxis>,
}

impl DeviceProfile {
    /// Returns the configured axis for the specified pedal, if any.
    #[must_use]
    pub fn get(&self, pedal: Pedal) -> Option<&PedalAxis> {
        match pedal {
            Pedal::Throttle => self.throttle.as_ref(),
            Pedal::Brake => self.brake.as_ref(),
            Pedal::Clutch => self.clutch.as_ref(),
        }
    }

    /// Sets or clears the configured axis for the specified pedal.
    pub fn set(&mut self, pedal: Pedal, axis: Option<PedalAxis>) {
        match pedal {
            Pedal::Throttle => self.throttle = axis,
            Pedal::Brake => self.brake = axis,
            Pedal::Clutch => self.clutch = axis,
        }
    }

    /// Returns `true` if none of the pedals are configured.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.throttle.is_none() && self.brake.is_none() && self.clutch.is_none()
    }
}

/// Errors that can occur during profile store operations.
#[derive(Debug)]
pub enum ProfileError {
    /// An underlying SQLite database error occurred.
    Sqlite(rusqlite::Error),
    /// A JSON serialization or deserialization error occurred.
    Json(serde_json::Error),
    /// A filesystem I/O error occurred.
    Io(std::io::Error),
}

impl std::fmt::Display for ProfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sqlite(err) => write!(f, "database error: {err}"),
            Self::Json(err) => write!(f, "json serialization error: {err}"),
            Self::Io(err) => write!(f, "io error: {err}"),
        }
    }
}

impl std::error::Error for ProfileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Sqlite(err) => Some(err),
            Self::Json(err) => Some(err),
            Self::Io(err) => Some(err),
        }
    }
}

impl From<rusqlite::Error> for ProfileError {
    fn from(err: rusqlite::Error) -> Self {
        Self::Sqlite(err)
    }
}

impl From<serde_json::Error> for ProfileError {
    fn from(err: serde_json::Error) -> Self {
        Self::Json(err)
    }
}

impl From<std::io::Error> for ProfileError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

/// Applies pending schema migrations inside a transaction.
fn apply_migrations(conn: &mut rusqlite::Connection) -> Result<(), ProfileError> {
    let current_version: u32 = conn.query_row("PRAGMA user_version;", [], |row| row.get(0))?;
    let start_idx = usize::try_from(current_version).unwrap_or(usize::MAX);

    if start_idx < MIGRATIONS.len() {
        let tx = conn.transaction()?;
        for (idx, sql) in MIGRATIONS.iter().enumerate().skip(start_idx) {
            tx.execute_batch(sql)?;
            let new_version = u32::try_from(idx + 1).unwrap_or(u32::MAX);
            tx.pragma_update(None, "user_version", new_version)?;
        }
        tx.commit()?;
    }

    Ok(())
}

/// SQLite-backed persistent store for device pedal profiles.
#[derive(Debug)]
pub struct ProfileStore {
    conn: rusqlite::Connection,
}

impl ProfileStore {
    /// Opens or creates a profile database at the specified filesystem path and runs migrations.
    ///
    /// Any missing parent directories will be created automatically.
    ///
    /// # Errors
    ///
    /// Returns [`ProfileError::Io`] if creating parent directories fails, or
    /// [`ProfileError::Sqlite`] if opening the database or applying migrations fails.
    pub fn open(path: &Path) -> Result<Self, ProfileError> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }
        let mut conn = rusqlite::Connection::open(path)?;
        apply_migrations(&mut conn)?;
        Ok(Self { conn })
    }

    /// Opens an in-memory profile database and runs migrations.
    ///
    /// # Errors
    ///
    /// Returns [`ProfileError::Sqlite`] if opening the database or applying migrations fails.
    pub fn open_in_memory() -> Result<Self, ProfileError> {
        let mut conn = rusqlite::Connection::open_in_memory()?;
        apply_migrations(&mut conn)?;
        Ok(Self { conn })
    }

    /// Loads the stored pedal profile for a device, if present.
    ///
    /// # Errors
    ///
    /// Returns [`ProfileError::Sqlite`] if querying the database fails, or
    /// [`ProfileError::Json`] if deserializing the stored profile fails.
    pub fn load(&self, key: &DeviceKey) -> Result<Option<DeviceProfile>, ProfileError> {
        let result = self.conn.query_row(
            "SELECT profile_json FROM device_profile WHERE guid = ?1 AND axis_count = ?2 AND button_count = ?3;",
            rusqlite::params![&key.guid, key.axis_count, key.button_count],
            |row| row.get::<_, String>(0),
        );
        match result {
            Ok(json_str) => {
                let profile: DeviceProfile = serde_json::from_str(&json_str)?;
                Ok(Some(profile))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(err) => Err(ProfileError::Sqlite(err)),
        }
    }

    /// Upserts a pedal profile for a device, recording the current timestamp.
    ///
    /// # Errors
    ///
    /// Returns [`ProfileError::Json`] if serializing the profile fails, or
    /// [`ProfileError::Sqlite`] if executing the database upsert fails.
    pub fn save(&self, key: &DeviceKey, profile: &DeviceProfile) -> Result<(), ProfileError> {
        let profile_json = serde_json::to_string(profile)?;
        let updated_at = i64::try_from(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        )
        .unwrap_or(i64::MAX);

        self.conn.execute(
            "INSERT INTO device_profile (guid, axis_count, button_count, profile_json, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5) \
             ON CONFLICT (guid, axis_count, button_count) DO UPDATE SET \
                 profile_json = excluded.profile_json, \
                 updated_at = excluded.updated_at;",
            rusqlite::params![
                &key.guid,
                key.axis_count,
                key.button_count,
                profile_json,
                updated_at
            ],
        )?;
        Ok(())
    }

    /// Deletes the pedal profile for a device, returning whether a row was removed.
    ///
    /// # Errors
    ///
    /// Returns [`ProfileError::Sqlite`] if executing the delete statement fails.
    pub fn delete(&self, key: &DeviceKey) -> Result<bool, ProfileError> {
        let rows_affected = self.conn.execute(
            "DELETE FROM device_profile WHERE guid = ?1 AND axis_count = ?2 AND button_count = ?3;",
            rusqlite::params![&key.guid, key.axis_count, key.button_count],
        )?;
        Ok(rows_affected > 0)
    }

    /// Lists all stored device profiles alongside their device keys.
    ///
    /// # Errors
    ///
    /// Returns [`ProfileError::Sqlite`] if querying the database fails, or
    /// [`ProfileError::Json`] if deserializing any stored profile fails.
    pub fn list(&self) -> Result<Vec<(DeviceKey, DeviceProfile)>, ProfileError> {
        let mut stmt = self.conn.prepare(
            "SELECT guid, axis_count, button_count, profile_json \
             FROM device_profile \
             ORDER BY guid, axis_count, button_count;",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, u32>(1)?,
                row.get::<_, u32>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;

        let mut results = Vec::new();
        for row in rows {
            let (guid, axis_count, button_count, profile_json) = row?;
            let profile: DeviceProfile = serde_json::from_str(&profile_json)?;
            results.push((
                DeviceKey {
                    guid,
                    axis_count,
                    button_count,
                },
                profile,
            ));
        }
        Ok(results)
    }

    /// Returns the database schema `user_version`.
    ///
    /// # Errors
    ///
    /// Returns [`ProfileError::Sqlite`] if querying the PRAGMA fails.
    pub fn user_version(&self) -> Result<u32, ProfileError> {
        self.conn
            .query_row("PRAGMA user_version;", [], |row| row.get(0))
            .map_err(ProfileError::Sqlite)
    }

    /// Applies any pending database schema migrations.
    ///
    /// # Errors
    ///
    /// Returns [`ProfileError::Sqlite`] if running a migration fails.
    pub fn apply_migrations(&mut self) -> Result<(), ProfileError> {
        apply_migrations(&mut self.conn)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempFileGuard(std::path::PathBuf);

    impl Drop for TempFileGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn load_missing_key_returns_none() {
        let store = ProfileStore::open_in_memory().unwrap();
        let key = DeviceKey::new("missing-device", 3, 10);
        assert_eq!(store.load(&key).unwrap(), None);
    }

    #[test]
    fn save_then_load_roundtrip() {
        let store = ProfileStore::open_in_memory().unwrap();
        let key = DeviceKey::new("roundtrip-device", 3, 10);
        let mut profile = DeviceProfile::default();
        let axis = PedalAxis {
            axis: 0,
            calibration: crate::calibration::AxisCalibration {
                min: -30000,
                max: 30000,
                invert: false,
                deadzone_low: 0.05,
                deadzone_high: 0.05,
            },
        };
        profile.set(Pedal::Throttle, Some(axis));
        store.save(&key, &profile).unwrap();

        let loaded = store.load(&key).unwrap().expect("profile should exist");
        assert_eq!(loaded, profile);
        assert_eq!(loaded.get(Pedal::Throttle), Some(&axis));
        assert_eq!(loaded.get(Pedal::Brake), None);
        assert!(!loaded.is_empty());
    }

    #[test]
    fn save_twice_second_wins_and_single_row() {
        let store = ProfileStore::open_in_memory().unwrap();
        let key = DeviceKey::new("device-1", 3, 10);
        let mut profile1 = DeviceProfile::default();
        profile1.set(
            Pedal::Throttle,
            Some(PedalAxis {
                axis: 0,
                calibration: crate::calibration::FULL_RANGE,
            }),
        );
        store.save(&key, &profile1).unwrap();

        let mut profile2 = DeviceProfile::default();
        profile2.set(
            Pedal::Brake,
            Some(PedalAxis {
                axis: 1,
                calibration: crate::calibration::FULL_RANGE,
            }),
        );
        store.save(&key, &profile2).unwrap();

        let loaded = store.load(&key).unwrap().expect("profile should exist");
        assert_eq!(loaded, profile2);

        let list = store.list().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].0, key);
        assert_eq!(list[0].1, profile2);
    }

    #[test]
    fn two_keys_with_same_guid_different_axis_counts_stay_separate() {
        let store = ProfileStore::open_in_memory().unwrap();
        let key1 = DeviceKey::new("fanatec-guid", 2, 12);
        let key2 = DeviceKey::new("fanatec-guid", 3, 12);

        let mut profile1 = DeviceProfile::default();
        profile1.set(
            Pedal::Throttle,
            Some(PedalAxis {
                axis: 0,
                calibration: crate::calibration::FULL_RANGE,
            }),
        );

        let mut profile2 = DeviceProfile::default();
        profile2.set(
            Pedal::Brake,
            Some(PedalAxis {
                axis: 1,
                calibration: crate::calibration::FULL_RANGE,
            }),
        );

        store.save(&key1, &profile1).unwrap();
        store.save(&key2, &profile2).unwrap();

        assert_eq!(store.load(&key1).unwrap(), Some(profile1));
        assert_eq!(store.load(&key2).unwrap(), Some(profile2));

        let list = store.list().unwrap();
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn delete_returns_true_then_false() {
        let store = ProfileStore::open_in_memory().unwrap();
        let key = DeviceKey::new("device-1", 3, 10);
        let profile = DeviceProfile::default();
        store.save(&key, &profile).unwrap();

        assert!(store.delete(&key).unwrap());
        assert!(!store.delete(&key).unwrap());
        assert_eq!(store.load(&key).unwrap(), None);
    }

    #[test]
    fn list() {
        let store = ProfileStore::open_in_memory().unwrap();
        assert_eq!(store.list().unwrap(), []);

        let key_a = DeviceKey::new("guid-a", 1, 0);
        let key_b = DeviceKey::new("guid-b", 2, 0);
        let p_a = DeviceProfile {
            throttle: Some(PedalAxis {
                axis: 0,
                calibration: crate::calibration::FULL_RANGE,
            }),
            brake: None,
            clutch: None,
        };
        let p_b = DeviceProfile {
            throttle: None,
            brake: Some(PedalAxis {
                axis: 1,
                calibration: crate::calibration::FULL_RANGE,
            }),
            clutch: None,
        };

        store.save(&key_a, &p_a).unwrap();
        store.save(&key_b, &p_b).unwrap();

        let list = store.list().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0], (key_a, p_a));
        assert_eq!(list[1], (key_b, p_b));
    }

    #[test]
    fn reopening_file_db_keeps_data_and_user_version_latest() {
        let temp_file = std::env::temp_dir().join(format!(
            "sct_test_profile_store_{}_{}.db",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let _guard = TempFileGuard(temp_file.clone());

        let key = DeviceKey::new("file-persisted-device", 3, 8);
        let mut profile = DeviceProfile::default();
        profile.set(
            Pedal::Clutch,
            Some(PedalAxis {
                axis: 2,
                calibration: crate::calibration::FULL_RANGE,
            }),
        );

        {
            let store = ProfileStore::open(&temp_file).unwrap();
            store.save(&key, &profile).unwrap();
            assert_eq!(store.user_version().unwrap(), 1);
        }

        {
            let store2 = ProfileStore::open(&temp_file).unwrap();
            assert_eq!(store2.load(&key).unwrap(), Some(profile));
            assert_eq!(store2.user_version().unwrap(), 1);
        }
    }

    #[test]
    fn running_migrations_twice_is_noop() {
        let mut store = ProfileStore::open_in_memory().unwrap();
        assert_eq!(store.user_version().unwrap(), 1);
        store.apply_migrations().unwrap();
        assert_eq!(store.user_version().unwrap(), 1);
    }

    #[test]
    fn device_profile_get_set_is_empty() {
        let mut profile = DeviceProfile::default();
        assert!(profile.is_empty());
        assert_eq!(profile.get(Pedal::Throttle), None);
        assert_eq!(profile.get(Pedal::Brake), None);
        assert_eq!(profile.get(Pedal::Clutch), None);

        let axis = PedalAxis {
            axis: 3,
            calibration: crate::calibration::FULL_RANGE,
        };

        for pedal in Pedal::ALL {
            profile.set(pedal, Some(axis));
            assert_eq!(profile.get(pedal), Some(&axis));
            assert!(!profile.is_empty());
            profile.set(pedal, None);
            assert_eq!(profile.get(pedal), None);
            assert!(profile.is_empty());
        }
    }

    #[test]
    fn serde_camel_case_roundtrip() {
        let key = DeviceKey::new("test-guid", 4, 16);
        let key_val = serde_json::to_value(&key).unwrap();
        assert_eq!(key_val["guid"], "test-guid");
        assert_eq!(key_val["axisCount"], 4);
        assert_eq!(key_val["buttonCount"], 16);
        let key_de: DeviceKey = serde_json::from_value(key_val).unwrap();
        assert_eq!(key, key_de);

        let pedal = Pedal::Throttle;
        let pedal_val = serde_json::to_value(pedal).unwrap();
        assert_eq!(pedal_val, "throttle");
        let pedal_de: Pedal = serde_json::from_value(pedal_val).unwrap();
        assert_eq!(pedal, pedal_de);

        let p_axis = PedalAxis {
            axis: 1,
            calibration: crate::calibration::FULL_RANGE,
        };
        let p_axis_val = serde_json::to_value(p_axis).unwrap();
        assert_eq!(p_axis_val["axis"], 1);
        assert!(p_axis_val.get("calibration").is_some());
        let p_axis_de: PedalAxis = serde_json::from_value(p_axis_val).unwrap();
        assert_eq!(p_axis, p_axis_de);

        let mut profile = DeviceProfile::default();
        profile.set(Pedal::Throttle, Some(p_axis));
        let profile_val = serde_json::to_value(&profile).unwrap();
        assert!(profile_val.get("throttle").is_some());
        assert_eq!(profile_val["brake"], serde_json::Value::Null);
        assert_eq!(profile_val["clutch"], serde_json::Value::Null);
        let profile_de: DeviceProfile = serde_json::from_value(profile_val).unwrap();
        assert_eq!(profile, profile_de);
    }

    #[test]
    fn error_display_and_source() {
        use std::error::Error;

        let io_err = ProfileError::from(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "file not found",
        ));
        assert!(io_err.to_string().contains("io error"));
        assert!(io_err.source().is_some());

        let json_raw: Result<DeviceProfile, serde_json::Error> =
            serde_json::from_str("invalid json");
        let json_err = ProfileError::from(json_raw.unwrap_err());
        assert!(json_err.to_string().contains("json serialization error"));
        assert!(json_err.source().is_some());

        let sqlite_raw = rusqlite::Connection::open_in_memory()
            .unwrap()
            .execute("INVALID SQL", []);
        let sqlite_err = ProfileError::from(sqlite_raw.unwrap_err());
        assert!(sqlite_err.to_string().contains("database error"));
        assert!(sqlite_err.source().is_some());
    }
}
