//! SQLite schema migrations and shared database configuration.

use rusqlite::Connection;

/// Migrations to be applied to the database in order.
///
/// - Migration 1: Device pedal profile table (`device_profile`, SCT-015).
/// - Migration 2: Attempt parent and child rep tables (`attempt`, `attempt_rep`, SCT-050).
///   `attempt.samples` is reserved for the compressed sample blob used by replays (not written yet).
/// - Migration 3: warm-up runs and their steps (`warm_up_run`, `warm_up_step`, SCT-045).
pub const MIGRATIONS: &[&str] = &[
    "CREATE TABLE device_profile (\
        guid TEXT NOT NULL, \
        axis_count INTEGER NOT NULL, \
        button_count INTEGER NOT NULL, \
        profile_json TEXT NOT NULL, \
        updated_at INTEGER NOT NULL, \
        PRIMARY KEY (guid, axis_count, button_count)\
    );",
    "CREATE TABLE attempt (\
        id INTEGER PRIMARY KEY AUTOINCREMENT, \
        drill_id TEXT NOT NULL, \
        preset_id TEXT NOT NULL, \
        pedal TEXT NOT NULL, \
        started_at TEXT NOT NULL, \
        aborted INTEGER NOT NULL, \
        best REAL, \
        average REAL, \
        consistency REAL, \
        samples BLOB\
    ); \
    CREATE INDEX idx_attempt_drill_started ON attempt (drill_id, started_at DESC, id DESC); \
    CREATE TABLE attempt_rep (\
        attempt_id INTEGER NOT NULL REFERENCES attempt (id) ON DELETE CASCADE, \
        rep_index INTEGER NOT NULL, \
        total REAL NOT NULL, \
        accuracy REAL NOT NULL, \
        timing REAL NOT NULL, \
        smoothness REAL NOT NULL, \
        time_in_band REAL, \
        rmse REAL, \
        overshoot REAL, \
        time_to_band_ms REAL, \
        jitter REAL, \
        lag_ms REAL, \
        ldlj_user REAL, \
        ldlj_target REAL, \
        PRIMARY KEY (attempt_id, rep_index)\
    );",
    "CREATE TABLE warm_up_run (\
        id INTEGER PRIMARY KEY AUTOINCREMENT, \
        preset_id TEXT NOT NULL, \
        started_at TEXT NOT NULL, \
        score REAL NOT NULL\
    ); \
    CREATE INDEX idx_warm_up_run_preset_score ON warm_up_run (preset_id, score DESC, started_at DESC); \
    CREATE TABLE warm_up_step (\
        run_id INTEGER NOT NULL REFERENCES warm_up_run (id) ON DELETE CASCADE, \
        step_index INTEGER NOT NULL, \
        drill_id TEXT NOT NULL, \
        skipped INTEGER NOT NULL, \
        attempt_id INTEGER REFERENCES attempt (id) ON DELETE SET NULL, \
        score REAL, \
        PRIMARY KEY (run_id, step_index)\
    );",
];

/// Applies pending schema migrations inside a transaction.
///
/// If `PRAGMA user_version` is 0 and the `device_profile` table already exists,
/// the database was created prior to schema versioning (migration #1). In that case,
/// version is set to 1 before applying any subsequent migrations.
///
/// # Errors
///
/// Returns [`rusqlite::Error`] if reading or writing pragma or executing migration SQL fails.
pub fn apply_migrations(conn: &mut Connection) -> Result<(), rusqlite::Error> {
    let mut current_version: u32 = conn.query_row("PRAGMA user_version;", [], |row| row.get(0))?;

    if current_version == 0 {
        let table_exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'device_profile');",
            [],
            |row| row.get(0),
        )?;
        if table_exists {
            current_version = 1;
            conn.pragma_update(None, "user_version", 1)?;
        }
    }

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

/// Returns the current schema version from `PRAGMA user_version`.
///
/// # Errors
///
/// Returns [`rusqlite::Error`] if the PRAGMA query fails.
pub fn user_version(conn: &Connection) -> Result<u32, rusqlite::Error> {
    conn.query_row("PRAGMA user_version;", [], |row| row.get(0))
}

/// Configures standard pragmas on a SQLite connection (foreign keys and busy timeout).
///
/// # Errors
///
/// Returns [`rusqlite::Error`] if setting PRAGMA directives fails.
pub fn configure_connection(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_db_runs_all_migrations() {
        let mut conn = Connection::open_in_memory().unwrap();
        configure_connection(&conn).unwrap();
        assert_eq!(user_version(&conn).unwrap(), 0);

        apply_migrations(&mut conn).unwrap();
        assert_eq!(
            user_version(&conn).unwrap(),
            u32::try_from(MIGRATIONS.len()).unwrap()
        );

        // Verify tables exist
        let profile_exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'device_profile');",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(profile_exists);

        let attempt_exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'attempt');",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(attempt_exists);

        let attempt_rep_exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'attempt_rep');",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(attempt_rep_exists);

        let warm_up_run_exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'warm_up_run');",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(warm_up_run_exists);

        let warm_up_step_exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'warm_up_step');",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(warm_up_step_exists);
    }

    #[test]
    fn existing_db_with_profile_table_and_user_version_0_upgrades_cleanly() {
        let mut conn = Connection::open_in_memory().unwrap();
        configure_connection(&conn).unwrap();

        // Simulate legacy DB: device_profile created manually/earlier with user_version = 0.
        conn.execute_batch(
            "CREATE TABLE device_profile (\
                guid TEXT NOT NULL, \
                axis_count INTEGER NOT NULL, \
                button_count INTEGER NOT NULL, \
                profile_json TEXT NOT NULL, \
                updated_at INTEGER NOT NULL, \
                PRIMARY KEY (guid, axis_count, button_count)\
            );",
        )
        .unwrap();
        assert_eq!(user_version(&conn).unwrap(), 0);

        // Insert legacy data to ensure it is not destroyed
        conn.execute(
            "INSERT INTO device_profile (guid, axis_count, button_count, profile_json, updated_at) \
             VALUES ('legacy-guid', 3, 10, '{}', 12345);",
            [],
        )
        .unwrap();

        // Migration runner must detect legacy table, upgrade to 1, and apply migration 2 cleanly
        apply_migrations(&mut conn).unwrap();
        assert_eq!(
            user_version(&conn).unwrap(),
            u32::try_from(MIGRATIONS.len()).unwrap()
        );

        // Verify legacy profile data still exists
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM device_profile WHERE guid = 'legacy-guid';",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        // Verify attempt table was created
        let attempt_exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'attempt');",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(attempt_exists);
    }

    #[test]
    fn running_migrations_twice_is_noop() {
        let mut conn = Connection::open_in_memory().unwrap();
        configure_connection(&conn).unwrap();
        apply_migrations(&mut conn).unwrap();
        let expected_version = u32::try_from(MIGRATIONS.len()).unwrap();
        assert_eq!(user_version(&conn).unwrap(), expected_version);

        apply_migrations(&mut conn).unwrap();
        assert_eq!(user_version(&conn).unwrap(), expected_version);
    }

    #[test]
    fn upgrade_from_version_2_keeps_attempts() {
        let mut conn = Connection::open_in_memory().unwrap();
        configure_connection(&conn).unwrap();

        // Build a DB by running only MIGRATIONS[0] and MIGRATIONS[1] and setting user_version = 2
        conn.execute_batch(MIGRATIONS[0]).unwrap();
        conn.execute_batch(MIGRATIONS[1]).unwrap();
        conn.pragma_update(None, "user_version", 2).unwrap();
        assert_eq!(user_version(&conn).unwrap(), 2);

        // Insert an attempt row
        conn.execute(
            "INSERT INTO attempt (drill_id, preset_id, pedal, started_at, aborted) \
             VALUES ('drill-test', 'gt3', 'brake', '2026-10-10T12:00:00Z', 0);",
            [],
        )
        .unwrap();

        // Run apply_migrations
        apply_migrations(&mut conn).unwrap();

        // Assert version == 3
        assert_eq!(user_version(&conn).unwrap(), 3);

        // Assert the attempt row survives
        let attempt_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM attempt WHERE drill_id = 'drill-test';",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(attempt_count, 1);

        // Assert both new tables exist
        let warm_up_run_exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'warm_up_run');",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(warm_up_run_exists);

        let warm_up_step_exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'warm_up_step');",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(warm_up_step_exists);
    }
}
