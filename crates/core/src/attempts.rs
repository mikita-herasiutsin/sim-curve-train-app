//! Persistent storage for drill set attempts and individual repetition scores.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::profile::Pedal;

/// Detailed scores and sub-scores for a single drill repetition in an attempt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttemptRep {
    /// 0-based index of this repetition within the set.
    pub rep_index: u32,
    /// Overall composite repetition score in `0.0..=100.0`.
    pub total: f32,
    /// Accuracy sub-score in `0.0..=100.0`.
    pub accuracy: f32,
    /// Timing / reaction latency sub-score in `0.0..=100.0`.
    pub timing: f32,
    /// Smoothness sub-score in `0.0..=100.0`.
    pub smoothness: f32,
    /// Time-weighted fraction of settled/drill duration spent inside tolerance band (`0.0..=1.0`).
    pub time_in_band: Option<f32>,
    /// Root-mean-square error from target position.
    pub rmse: Option<f32>,
    /// Maximum excursion / overshoot beyond target maximum or tolerance band.
    pub overshoot: Option<f32>,
    /// Hold-drill specific: time in milliseconds until first entering the tolerance band.
    pub time_to_band_ms: Option<f32>,
    /// Hold-drill specific: RMS jitter deviation from 50 ms moving average.
    pub jitter: Option<f32>,
    /// Trace-drill specific: estimated driver reaction lag in milliseconds.
    pub lag_ms: Option<f32>,
    /// Trace-drill specific: log dimensionless jerk of the user's filtered trace.
    pub ldlj_user: Option<f32>,
    /// Trace-drill specific: log dimensionless jerk of the reference target curve.
    pub ldlj_target: Option<f32>,
}

impl AttemptRep {
    /// Creates a rep score record from a completed hold drill repetition.
    #[must_use]
    pub fn from_hold(rep_index: u32, score: &crate::scoring::HoldScore) -> Self {
        Self {
            rep_index,
            total: score.total,
            accuracy: score.accuracy,
            timing: score.timing,
            smoothness: score.smoothness,
            time_in_band: Some(score.time_in_band),
            rmse: Some(score.rmse),
            overshoot: Some(score.overshoot),
            time_to_band_ms: score.time_to_band_ms,
            jitter: Some(score.jitter),
            lag_ms: None,
            ldlj_user: None,
            ldlj_target: None,
        }
    }

    /// Creates a rep score record from a completed trace drill repetition.
    #[must_use]
    pub fn from_trace(rep_index: u32, score: &crate::trace_scoring::TraceScore) -> Self {
        Self {
            rep_index,
            total: score.total,
            accuracy: score.accuracy,
            timing: score.timing,
            smoothness: score.smoothness,
            time_in_band: Some(score.time_in_band),
            rmse: Some(score.rmse),
            overshoot: Some(score.overshoot),
            time_to_band_ms: None,
            jitter: None,
            lag_ms: Some(score.lag_ms),
            ldlj_user: Some(score.ldlj_user),
            ldlj_target: Some(score.ldlj_target),
        }
    }
}

/// Data required to persist a new drill set attempt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewAttempt {
    /// Identifier of the drill executed.
    pub drill_id: String,
    /// Identifier of the parent preset containing the drill.
    pub preset_id: String,
    /// Pedal targeted by the drill.
    pub pedal: Pedal,
    /// UTC timestamp when the set started (formatted as an ISO 8601 string).
    pub started_at: String,
    /// Whether the set ended before all scheduled reps did (aborted, or the pedals stopped).
    pub aborted: bool,
    /// Highest rep total score achieved in this set.
    pub best: Option<f32>,
    /// Arithmetic mean of completed rep total scores.
    pub average: Option<f32>,
    /// Consistency sub-score in `0.0..=100.0` based on rep score spread.
    pub consistency: Option<f32>,
    /// Completed repetition scores.
    pub reps: Vec<AttemptRep>,
}

impl NewAttempt {
    /// Constructs a new attempt record, optionally populating summary statistics from [`crate::set_summary::SetSummary`].
    #[must_use]
    pub fn new(
        drill_id: impl Into<String>,
        preset_id: impl Into<String>,
        pedal: Pedal,
        started_at: impl Into<String>,
        aborted: bool,
        summary: Option<&crate::set_summary::SetSummary>,
        reps: Vec<AttemptRep>,
    ) -> Self {
        Self {
            drill_id: drill_id.into(),
            preset_id: preset_id.into(),
            pedal,
            started_at: started_at.into(),
            aborted,
            best: summary.map(|s| s.best),
            average: summary.map(|s| s.average),
            consistency: summary.and_then(|s| s.consistency),
            reps,
        }
    }
}

/// A persisted drill set attempt with its assigned unique ID and child reps.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attempt {
    /// Database primary key for this attempt.
    pub id: i64,
    /// Identifier of the drill executed.
    pub drill_id: String,
    /// Identifier of the parent preset containing the drill.
    pub preset_id: String,
    /// Pedal targeted by the drill.
    pub pedal: Pedal,
    /// UTC timestamp when the set started (formatted as an ISO 8601 string).
    pub started_at: String,
    /// Whether the set ended before all scheduled reps did (aborted, or the pedals stopped).
    pub aborted: bool,
    /// Highest rep total score achieved in this set.
    pub best: Option<f32>,
    /// Arithmetic mean of completed rep total scores.
    pub average: Option<f32>,
    /// Consistency sub-score in `0.0..=100.0` based on rep score spread.
    pub consistency: Option<f32>,
    /// Completed repetition scores.
    pub reps: Vec<AttemptRep>,
}

/// One step of a warm-up run, in run order. Mirrors the TS `WarmUpStepResult`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WarmUpStepResult {
    /// Identifier of the drill executed in this step.
    pub drill_id: String,
    /// Planned rep count of the step.
    pub reps: u32,
    /// True when the step was skipped, before or during its set.
    pub skipped: bool,
    /// The attempt saved for this step's set; None when no set was saved.
    pub attempt_id: Option<i64>,
    /// The set's average rep score; None when skipped or no rep was scored.
    pub score: Option<f32>,
}

/// Data required to persist a finished warm-up run.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewWarmUpRun {
    /// Identifier of the parent preset containing the warm-up drills.
    pub preset_id: String,
    /// UTC timestamp when the warm-up run started (formatted as an ISO 8601 string).
    pub started_at: String,
    /// Step results in run order.
    pub steps: Vec<WarmUpStepResult>,
}

/// A persisted warm-up run with its assigned unique ID, overall score, and child steps.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WarmUpRun {
    /// Database primary key for this warm-up run.
    pub id: i64,
    /// Identifier of the parent preset containing the warm-up drills.
    pub preset_id: String,
    /// UTC timestamp when the warm-up run started (formatted as an ISO 8601 string).
    pub started_at: String,
    /// Overall warm-up score computed from the step results.
    pub score: f32,
    /// Step results in run order.
    pub steps: Vec<WarmUpStepResult>,
}

/// Overall warm-up score (D-26): the mean of the steps' set averages, where a skipped step or
/// a step without a score counts as 0. An empty run scores 0.
#[must_use]
#[expect(clippy::cast_precision_loss, reason = "step count fits in f32")]
pub fn warm_up_score(steps: &[WarmUpStepResult]) -> f32 {
    if steps.is_empty() {
        return 0.0;
    }
    let total: f32 = steps
        .iter()
        .map(|s| {
            if s.skipped {
                0.0
            } else {
                s.score.unwrap_or(0.0)
            }
        })
        .sum();
    total / steps.len() as f32
}

/// Errors that can occur during attempt store operations.
#[derive(Debug)]
pub enum AttemptError {
    /// An underlying SQLite database error occurred.
    Sqlite(rusqlite::Error),
    /// A filesystem I/O error occurred.
    Io(std::io::Error),
    /// The data to save is invalid. Nothing was written.
    Invalid(String),
}

impl std::fmt::Display for AttemptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sqlite(err) => write!(f, "database error: {err}"),
            Self::Io(err) => write!(f, "io error: {err}"),
            Self::Invalid(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for AttemptError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Sqlite(err) => Some(err),
            Self::Io(err) => Some(err),
            Self::Invalid(_) => None,
        }
    }
}

impl From<rusqlite::Error> for AttemptError {
    fn from(err: rusqlite::Error) -> Self {
        Self::Sqlite(err)
    }
}

impl From<std::io::Error> for AttemptError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

/// SQLite-backed persistent store for drill attempts and repetition scores.
#[derive(Debug)]
pub struct AttemptStore {
    conn: rusqlite::Connection,
}

impl AttemptStore {
    /// Opens or creates an attempts database at the specified filesystem path and runs migrations.
    ///
    /// Any missing parent directories will be created automatically.
    ///
    /// # Errors
    ///
    /// Returns [`AttemptError::Io`] if creating parent directories fails, or
    /// [`AttemptError::Sqlite`] if opening the database or applying migrations fails.
    pub fn open(path: &Path) -> Result<Self, AttemptError> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }
        let mut conn = rusqlite::Connection::open(path)?;
        crate::db::configure_connection(&conn)?;
        crate::db::apply_migrations(&mut conn)?;
        Ok(Self { conn })
    }

    /// Opens an in-memory attempts database and runs migrations.
    ///
    /// # Errors
    ///
    /// Returns [`AttemptError::Sqlite`] if opening the database or applying migrations fails.
    pub fn open_in_memory() -> Result<Self, AttemptError> {
        let mut conn = rusqlite::Connection::open_in_memory()?;
        crate::db::configure_connection(&conn)?;
        crate::db::apply_migrations(&mut conn)?;
        Ok(Self { conn })
    }

    /// Returns the database schema `user_version`.
    ///
    /// # Errors
    ///
    /// Returns [`AttemptError::Sqlite`] if querying the PRAGMA fails.
    pub fn user_version(&self) -> Result<u32, AttemptError> {
        crate::db::user_version(&self.conn).map_err(AttemptError::Sqlite)
    }

    /// Applies any pending database schema migrations.
    ///
    /// # Errors
    ///
    /// Returns [`AttemptError::Sqlite`] if running a migration fails.
    pub fn apply_migrations(&mut self) -> Result<(), AttemptError> {
        crate::db::apply_migrations(&mut self.conn).map_err(AttemptError::Sqlite)
    }

    /// Persists a completed or aborted attempt set and all of its child reps.
    ///
    /// Returns the database generated unique ID for the newly saved attempt.
    ///
    /// # Errors
    ///
    /// Returns [`AttemptError::Sqlite`] if executing any insert statement fails.
    pub fn save_attempt(&self, attempt: &NewAttempt) -> Result<i64, AttemptError> {
        let best = attempt
            .best
            .or_else(|| attempt.reps.iter().map(|r| r.total).max_by(f32::total_cmp));

        // Rolls back on drop, so an error (or a failed commit) never leaves the transaction open.
        let tx = self.conn.unchecked_transaction()?;

        let result = (|| -> Result<i64, AttemptError> {
            self.conn.execute(
                "INSERT INTO attempt (drill_id, preset_id, pedal, started_at, aborted, best, average, consistency) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8);",
                rusqlite::params![
                    &attempt.drill_id,
                    &attempt.preset_id,
                    attempt.pedal.as_str(),
                    &attempt.started_at,
                    i64::from(attempt.aborted),
                    best,
                    attempt.average,
                    attempt.consistency,
                ],
            )?;

            let attempt_id = self.conn.last_insert_rowid();

            let mut rep_stmt = self.conn.prepare(
                "INSERT INTO attempt_rep (attempt_id, rep_index, total, accuracy, timing, smoothness, \
                 time_in_band, rmse, overshoot, time_to_band_ms, jitter, lag_ms, ldlj_user, ldlj_target) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14);",
            )?;

            for rep in &attempt.reps {
                rep_stmt.execute(rusqlite::params![
                    attempt_id,
                    rep.rep_index,
                    rep.total,
                    rep.accuracy,
                    rep.timing,
                    rep.smoothness,
                    rep.time_in_band,
                    rep.rmse,
                    rep.overshoot,
                    rep.time_to_band_ms,
                    rep.jitter,
                    rep.lag_ms,
                    rep.ldlj_user,
                    rep.ldlj_target,
                ])?;
            }

            Ok(attempt_id)
        })();

        let id = result?;
        tx.commit()?;
        Ok(id)
    }

    /// Lists the most recent recorded attempts for a drill, up to `limit`.
    ///
    /// Results are sorted newest first (`started_at DESC, id DESC`).
    ///
    /// # Errors
    ///
    /// Returns [`AttemptError::Sqlite`] if querying attempts or child reps fails.
    pub fn list_attempts(&self, drill_id: &str, limit: u32) -> Result<Vec<Attempt>, AttemptError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, drill_id, preset_id, pedal, started_at, aborted, best, average, consistency \
             FROM attempt \
             WHERE drill_id = ?1 \
             ORDER BY started_at DESC, id DESC \
             LIMIT ?2;",
        )?;

        let mut rep_stmt = self.conn.prepare(
            "SELECT rep_index, total, accuracy, timing, smoothness, \
                    time_in_band, rmse, overshoot, time_to_band_ms, jitter, \
                    lag_ms, ldlj_user, ldlj_target \
             FROM attempt_rep \
             WHERE attempt_id = ?1 \
             ORDER BY rep_index ASC;",
        )?;

        let rows = stmt.query_map(rusqlite::params![drill_id, limit], |row| {
            let id: i64 = row.get(0)?;
            let drill_id: String = row.get(1)?;
            let preset_id: String = row.get(2)?;
            let pedal_str: String = row.get(3)?;
            let pedal = Pedal::parse_str(&pedal_str).ok_or_else(|| {
                rusqlite::Error::FromSqlConversionFailure(
                    3,
                    rusqlite::types::Type::Text,
                    Box::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("invalid pedal: {pedal_str}"),
                    )),
                )
            })?;
            let started_at: String = row.get(4)?;
            let aborted_int: i64 = row.get(5)?;
            let aborted = aborted_int != 0;
            let best: Option<f32> = row.get(6)?;
            let average: Option<f32> = row.get(7)?;
            let consistency: Option<f32> = row.get(8)?;

            Ok((
                id,
                drill_id,
                preset_id,
                pedal,
                started_at,
                aborted,
                best,
                average,
                consistency,
            ))
        })?;

        let mut attempts = Vec::new();
        for row in rows {
            let (id, drill_id, preset_id, pedal, started_at, aborted, best, average, consistency) =
                row?;

            let rep_rows = rep_stmt.query_map(rusqlite::params![id], |rep_row| {
                Ok(AttemptRep {
                    rep_index: rep_row.get(0)?,
                    total: rep_row.get(1)?,
                    accuracy: rep_row.get(2)?,
                    timing: rep_row.get(3)?,
                    smoothness: rep_row.get(4)?,
                    time_in_band: rep_row.get(5)?,
                    rmse: rep_row.get(6)?,
                    overshoot: rep_row.get(7)?,
                    time_to_band_ms: rep_row.get(8)?,
                    jitter: rep_row.get(9)?,
                    lag_ms: rep_row.get(10)?,
                    ldlj_user: rep_row.get(11)?,
                    ldlj_target: rep_row.get(12)?,
                })
            })?;

            let mut reps = Vec::new();
            for rep in rep_rows {
                reps.push(rep?);
            }

            attempts.push(Attempt {
                id,
                drill_id,
                preset_id,
                pedal,
                started_at,
                aborted,
                best,
                average,
                consistency,
                reps,
            });
        }

        Ok(attempts)
    }

    /// Returns the highest total score recorded for a drill, or `None` if no attempts exist.
    ///
    /// # Errors
    ///
    /// Returns [`AttemptError::Sqlite`] if querying the database fails.
    pub fn best_total(&self, drill_id: &str) -> Result<Option<f32>, AttemptError> {
        let best: Option<f32> = self.conn.query_row(
            "SELECT MAX(best) FROM attempt WHERE drill_id = ?1;",
            rusqlite::params![drill_id],
            |row| row.get(0),
        )?;
        Ok(best)
    }

    /// Returns the highest total score per drill of a preset, keyed by drill id. Drills with no scored attempt are absent.
    ///
    /// # Errors
    ///
    /// Returns [`AttemptError::Sqlite`] if querying the database fails.
    pub fn best_totals(
        &self,
        preset_id: &str,
    ) -> Result<std::collections::HashMap<String, f32>, AttemptError> {
        let mut stmt = self.conn.prepare(
            "SELECT drill_id, MAX(best) FROM attempt WHERE preset_id = ?1 AND best IS NOT NULL GROUP BY drill_id;",
        )?;
        let totals = stmt
            .query_map(rusqlite::params![preset_id], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })?
            .collect::<Result<_, _>>()?;
        Ok(totals)
    }

    /// Persists a completed warm-up run and its individual step results.
    ///
    /// Computes the overall score from the steps via [`warm_up_score`].
    /// Returns the database generated unique ID for the newly saved warm-up run.
    ///
    /// A skipped step is stored with no score, whatever `score` the caller sent.
    ///
    /// # Errors
    ///
    /// Returns [`AttemptError::Invalid`] if `preset_id` is empty after trimming or `steps` is
    /// empty. Nothing is written in that case.
    ///
    /// Returns [`AttemptError::Sqlite`] if executing any insert statement fails or foreign key
    /// constraints are violated.
    pub fn save_warm_up_run(&self, run: &NewWarmUpRun) -> Result<i64, AttemptError> {
        if run.preset_id.trim().is_empty() {
            return Err(AttemptError::Invalid(
                "warm-up run preset_id must be non-empty".to_string(),
            ));
        }
        if run.steps.is_empty() {
            return Err(AttemptError::Invalid(
                "warm-up run must have at least one step".to_string(),
            ));
        }

        let score = warm_up_score(&run.steps);

        // Rolls back on drop, so an error (or a failed commit) never leaves the transaction open.
        let tx = self.conn.unchecked_transaction()?;

        let result = (|| -> Result<i64, AttemptError> {
            self.conn.execute(
                "INSERT INTO warm_up_run (preset_id, started_at, score) \
                 VALUES (?1, ?2, ?3);",
                rusqlite::params![&run.preset_id, &run.started_at, score],
            )?;

            let run_id = self.conn.last_insert_rowid();

            let mut step_stmt = self.conn.prepare(
                "INSERT INTO warm_up_step (run_id, step_index, drill_id, reps, skipped, attempt_id, score) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7);",
            )?;

            for (idx, step) in run.steps.iter().enumerate() {
                let step_index = i64::try_from(idx).unwrap_or(i64::MAX);
                let step_score = if step.skipped { None } else { step.score };
                step_stmt.execute(rusqlite::params![
                    run_id,
                    step_index,
                    &step.drill_id,
                    i64::from(step.reps),
                    i64::from(step.skipped),
                    step.attempt_id,
                    step_score,
                ])?;
            }

            Ok(run_id)
        })();

        let id = result?;
        tx.commit()?;
        Ok(id)
    }

    /// Lists the most recent recorded warm-up runs for a preset, up to `limit`.
    ///
    /// Results are sorted newest first (`started_at DESC, id DESC`), each with its steps
    /// ordered by `step_index`.
    ///
    /// # Errors
    ///
    /// Returns [`AttemptError::Sqlite`] if querying warm-up runs or child steps fails.
    pub fn list_warm_up_runs(
        &self,
        preset_id: &str,
        limit: u32,
    ) -> Result<Vec<WarmUpRun>, AttemptError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, preset_id, started_at, score \
             FROM warm_up_run \
             WHERE preset_id = ?1 \
             ORDER BY started_at DESC, id DESC \
             LIMIT ?2;",
        )?;

        let mut step_stmt = self.conn.prepare(
            "SELECT drill_id, reps, skipped, attempt_id, score \
             FROM warm_up_step \
             WHERE run_id = ?1 \
             ORDER BY step_index ASC;",
        )?;

        let rows = stmt.query_map(rusqlite::params![preset_id, limit], |row| {
            let id: i64 = row.get(0)?;
            let preset_id: String = row.get(1)?;
            let started_at: String = row.get(2)?;
            let score: f32 = row.get(3)?;
            Ok((id, preset_id, started_at, score))
        })?;

        let mut runs = Vec::new();
        for row in rows {
            let (id, preset_id, started_at, score) = row?;

            let step_rows = step_stmt.query_map(rusqlite::params![id], |step_row| {
                let drill_id: String = step_row.get(0)?;
                let reps: u32 = step_row.get(1)?;
                let skipped_int: i64 = step_row.get(2)?;
                let attempt_id: Option<i64> = step_row.get(3)?;
                let score: Option<f32> = step_row.get(4)?;

                Ok(WarmUpStepResult {
                    drill_id,
                    reps,
                    skipped: skipped_int != 0,
                    attempt_id,
                    score,
                })
            })?;

            let mut steps = Vec::new();
            for step in step_rows {
                steps.push(step?);
            }

            runs.push(WarmUpRun {
                id,
                preset_id,
                started_at,
                score,
                steps,
            });
        }

        Ok(runs)
    }

    #[cfg(test)]
    pub(crate) fn raw_conn(&self) -> &rusqlite::Connection {
        &self.conn
    }
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    struct TempFileGuard(std::path::PathBuf);

    impl Drop for TempFileGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    fn sample_reps() -> Vec<AttemptRep> {
        vec![
            AttemptRep {
                rep_index: 0,
                total: 88.5,
                accuracy: 90.0,
                timing: 85.0,
                smoothness: 89.0,
                time_in_band: Some(0.92),
                rmse: Some(0.015),
                overshoot: Some(0.02),
                time_to_band_ms: Some(180.0),
                jitter: Some(0.005),
                lag_ms: None,
                ldlj_user: None,
                ldlj_target: None,
            },
            AttemptRep {
                rep_index: 1,
                total: 94.0,
                accuracy: 95.0,
                timing: 92.0,
                smoothness: 94.0,
                time_in_band: Some(0.96),
                rmse: Some(0.010),
                overshoot: Some(0.01),
                time_to_band_ms: Some(160.0),
                jitter: Some(0.003),
                lag_ms: Some(25.0),
                ldlj_user: Some(-4.2),
                ldlj_target: Some(-4.0),
            },
        ]
    }

    #[test]
    fn temp_file_db_survives_close_and_reopen() {
        let temp_file = std::env::temp_dir().join(format!(
            "sct_test_attempt_store_{}_{}.db",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let _guard = TempFileGuard(temp_file.clone());

        let new_attempt = NewAttempt {
            drill_id: "hold-brake-70".to_string(),
            preset_id: "gt3".to_string(),
            pedal: Pedal::Brake,
            started_at: "2026-10-07T12:00:00Z".to_string(),
            aborted: false,
            best: Some(94.0),
            average: Some(91.25),
            consistency: Some(88.0),
            reps: sample_reps(),
        };

        let saved_id = {
            let store = AttemptStore::open(&temp_file).unwrap();
            assert_eq!(
                store.user_version().unwrap(),
                u32::try_from(crate::db::MIGRATIONS.len()).unwrap()
            );
            let id = store.save_attempt(&new_attempt).unwrap();
            assert!(id > 0);
            id
        };

        {
            let store = AttemptStore::open(&temp_file).unwrap();
            assert_eq!(
                store.user_version().unwrap(),
                u32::try_from(crate::db::MIGRATIONS.len()).unwrap()
            );
            let attempts = store.list_attempts("hold-brake-70", 10).unwrap();
            assert_eq!(attempts.len(), 1);

            let loaded = &attempts[0];
            assert_eq!(loaded.id, saved_id);
            assert_eq!(loaded.drill_id, new_attempt.drill_id);
            assert_eq!(loaded.preset_id, new_attempt.preset_id);
            assert_eq!(loaded.pedal, new_attempt.pedal);
            assert_eq!(loaded.started_at, new_attempt.started_at);
            assert_eq!(loaded.aborted, new_attempt.aborted);
            assert_eq!(loaded.best, new_attempt.best);
            assert_eq!(loaded.average, new_attempt.average);
            assert_eq!(loaded.consistency, new_attempt.consistency);
            assert_eq!(loaded.reps, new_attempt.reps);
        }
    }

    #[test]
    fn list_attempts_ordering_and_limit() {
        let store = AttemptStore::open_in_memory().unwrap();

        let make_attempt = |drill: &str, time: &str, best: f32| NewAttempt {
            drill_id: drill.to_string(),
            preset_id: "gt3".to_string(),
            pedal: Pedal::Brake,
            started_at: time.to_string(),
            aborted: false,
            best: Some(best),
            average: Some(best),
            consistency: None,
            reps: vec![AttemptRep {
                rep_index: 0,
                total: best,
                accuracy: best,
                timing: best,
                smoothness: best,
                time_in_band: None,
                rmse: None,
                overshoot: None,
                time_to_band_ms: None,
                jitter: None,
                lag_ms: None,
                ldlj_user: None,
                ldlj_target: None,
            }],
        };

        store
            .save_attempt(&make_attempt("drill-1", "2026-10-01T10:00:00Z", 75.0))
            .unwrap();
        store
            .save_attempt(&make_attempt("drill-1", "2026-10-03T10:00:00Z", 92.0))
            .unwrap();
        store
            .save_attempt(&make_attempt("drill-1", "2026-10-02T10:00:00Z", 85.0))
            .unwrap();
        store
            .save_attempt(&make_attempt("drill-2", "2026-10-04T10:00:00Z", 99.0))
            .unwrap();

        // Newest first order
        let list = store.list_attempts("drill-1", 10).unwrap();
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].started_at, "2026-10-03T10:00:00Z");
        assert_eq!(list[0].best, Some(92.0));
        assert_eq!(list[1].started_at, "2026-10-02T10:00:00Z");
        assert_eq!(list[1].best, Some(85.0));
        assert_eq!(list[2].started_at, "2026-10-01T10:00:00Z");
        assert_eq!(list[2].best, Some(75.0));

        // Limit works
        let limited = store.list_attempts("drill-1", 2).unwrap();
        assert_eq!(limited.len(), 2);
        assert_eq!(limited[0].started_at, "2026-10-03T10:00:00Z");
        assert_eq!(limited[1].started_at, "2026-10-02T10:00:00Z");

        // Different drill
        let drill2 = store.list_attempts("drill-2", 10).unwrap();
        assert_eq!(drill2.len(), 1);
        assert_eq!(drill2[0].best, Some(99.0));

        // Non-existent drill
        let empty = store.list_attempts("drill-none", 10).unwrap();
        assert_eq!(empty, []);
    }

    #[test]
    fn best_total_query() {
        let store = AttemptStore::open_in_memory().unwrap();

        assert_eq!(store.best_total("drill-1").unwrap(), None);

        let make_attempt = |drill: &str, best: f32| NewAttempt {
            drill_id: drill.to_string(),
            preset_id: "gt3".to_string(),
            pedal: Pedal::Throttle,
            started_at: "2026-10-07T12:00:00Z".to_string(),
            aborted: false,
            best: Some(best),
            average: Some(best),
            consistency: None,
            reps: Vec::new(),
        };

        store.save_attempt(&make_attempt("drill-1", 80.0)).unwrap();
        store.save_attempt(&make_attempt("drill-1", 95.5)).unwrap();
        store.save_attempt(&make_attempt("drill-1", 88.0)).unwrap();
        store.save_attempt(&make_attempt("drill-2", 98.0)).unwrap();

        assert_eq!(store.best_total("drill-1").unwrap(), Some(95.5));
        assert_eq!(store.best_total("drill-2").unwrap(), Some(98.0));
        assert_eq!(store.best_total("drill-3").unwrap(), None);
    }

    #[test]
    fn best_totals_per_preset() {
        let store = AttemptStore::open_in_memory().unwrap();

        let make_attempt = |preset: &str, drill: &str, best: Option<f32>| NewAttempt {
            drill_id: drill.to_string(),
            preset_id: preset.to_string(),
            pedal: Pedal::Throttle,
            started_at: "2026-10-07T12:00:00Z".to_string(),
            aborted: false,
            best,
            average: best,
            consistency: None,
            reps: Vec::new(),
        };

        store
            .save_attempt(&make_attempt("gt3", "d1", Some(80.0)))
            .unwrap();
        store
            .save_attempt(&make_attempt("gt3", "d1", Some(90.0)))
            .unwrap();
        store
            .save_attempt(&make_attempt("gt3", "d2", Some(70.0)))
            .unwrap();
        store
            .save_attempt(&make_attempt("mx5", "d1", Some(99.0)))
            .unwrap();
        store
            .save_attempt(&make_attempt("gt3", "d3", None))
            .unwrap();

        let gt3 = store.best_totals("gt3").unwrap();
        assert_eq!(gt3.len(), 2);
        assert_eq!(
            gt3,
            std::collections::HashMap::from([("d1".to_string(), 90.0), ("d2".to_string(), 70.0),])
        );

        let mx5 = store.best_totals("mx5").unwrap();
        assert_eq!(
            mx5,
            std::collections::HashMap::from([("d1".to_string(), 99.0)])
        );

        let unknown = store.best_totals("unknown").unwrap();
        assert!(unknown.is_empty());
    }

    #[test]
    fn aborted_attempt_with_no_reps() {
        let store = AttemptStore::open_in_memory().unwrap();

        let aborted = NewAttempt {
            drill_id: "drill-aborted".to_string(),
            preset_id: "mx5".to_string(),
            pedal: Pedal::Clutch,
            started_at: "2026-10-07T13:00:00Z".to_string(),
            aborted: true,
            best: None,
            average: None,
            consistency: None,
            reps: Vec::new(),
        };

        let id = store.save_attempt(&aborted).unwrap();
        assert!(id > 0);

        let list = store.list_attempts("drill-aborted", 10).unwrap();
        assert_eq!(list.len(), 1);
        assert!(list[0].aborted);
        assert_eq!(list[0].best, None);
        assert_eq!(list[0].reps, []);
        assert_eq!(store.best_total("drill-aborted").unwrap(), None);
    }

    #[test]
    fn helper_constructors_from_scores() {
        let hold_score = crate::scoring::HoldScore {
            total: 92.0,
            grade: crate::scoring::Grade::A,
            accuracy: 94.0,
            timing: 90.0,
            smoothness: 91.0,
            time_in_band: 0.95,
            rmse: 0.02,
            time_to_band_ms: Some(150.0),
            overshoot: 0.01,
            jitter: 0.004,
        };

        let hold_rep = AttemptRep::from_hold(0, &hold_score);
        assert_eq!(hold_rep.total, 92.0);
        assert_eq!(hold_rep.time_to_band_ms, Some(150.0));
        assert_eq!(hold_rep.lag_ms, None);

        let trace_score = crate::trace_scoring::TraceScore {
            total: 88.0,
            grade: crate::scoring::Grade::A,
            accuracy: 89.0,
            timing: 86.0,
            smoothness: 88.0,
            lag_ms: 30.0,
            time_in_band: 0.90,
            rmse: 0.03,
            overshoot: 0.02,
            ldlj_user: -5.0,
            ldlj_target: -4.8,
        };

        let trace_rep = AttemptRep::from_trace(1, &trace_score);
        assert_eq!(trace_rep.total, 88.0);
        assert_eq!(trace_rep.lag_ms, Some(30.0));
        assert_eq!(trace_rep.ldlj_user, Some(-5.0));

        let summary = crate::set_summary::summarize_set(&[92.0, 88.0]).unwrap();
        let new_attempt = NewAttempt::new(
            "trace-hairpin",
            "gt3",
            Pedal::Brake,
            "2026-10-07T14:00:00Z",
            false,
            Some(&summary),
            vec![hold_rep, trace_rep],
        );
        assert_eq!(new_attempt.best, Some(92.0));
        assert_eq!(new_attempt.average, Some(90.0));
        assert!(new_attempt.consistency.is_some());
        assert_eq!(new_attempt.reps.len(), 2);
    }

    #[test]
    fn warm_up_score_calculation() {
        assert_eq!(warm_up_score(&[]), 0.0);

        let steps = vec![
            WarmUpStepResult {
                drill_id: "d1".to_string(),
                reps: 10,
                skipped: false,
                attempt_id: Some(1),
                score: Some(80.0),
            },
            WarmUpStepResult {
                drill_id: "d2".to_string(),
                reps: 10,
                skipped: true,
                attempt_id: None,
                score: None,
            },
            WarmUpStepResult {
                drill_id: "d3".to_string(),
                reps: 10,
                skipped: false,
                attempt_id: Some(2),
                score: Some(60.0),
            },
            WarmUpStepResult {
                drill_id: "d4".to_string(),
                reps: 10,
                skipped: false,
                attempt_id: None,
                score: None,
            },
        ];
        assert_eq!(warm_up_score(&steps), 35.0);

        let all_scored = vec![
            WarmUpStepResult {
                drill_id: "d1".to_string(),
                reps: 10,
                skipped: false,
                attempt_id: Some(1),
                score: Some(80.0),
            },
            WarmUpStepResult {
                drill_id: "d2".to_string(),
                reps: 10,
                skipped: false,
                attempt_id: Some(2),
                score: Some(90.0),
            },
        ];
        assert_eq!(warm_up_score(&all_scored), 85.0);
    }

    #[test]
    fn warm_up_run_save_and_list_round_trip() {
        let store = AttemptStore::open_in_memory().unwrap();

        let attempt = NewAttempt {
            drill_id: "d1".to_string(),
            preset_id: "gt3".to_string(),
            pedal: Pedal::Brake,
            started_at: "2026-10-10T11:00:00Z".to_string(),
            aborted: false,
            best: Some(95.0),
            average: Some(95.0),
            consistency: None,
            reps: Vec::new(),
        };
        let saved_attempt_id = store.save_attempt(&attempt).unwrap();

        let new_run = NewWarmUpRun {
            preset_id: "gt3".to_string(),
            started_at: "2026-10-10T12:00:00Z".to_string(),
            steps: vec![
                WarmUpStepResult {
                    drill_id: "d1".to_string(),
                    reps: 3,
                    skipped: false,
                    attempt_id: Some(saved_attempt_id),
                    score: Some(95.0),
                },
                WarmUpStepResult {
                    drill_id: "d2".to_string(),
                    reps: 5,
                    skipped: true,
                    attempt_id: None,
                    score: None,
                },
                WarmUpStepResult {
                    drill_id: "d3".to_string(),
                    reps: 7,
                    skipped: false,
                    attempt_id: None,
                    score: Some(65.0),
                },
            ],
        };

        let expected_score = warm_up_score(&new_run.steps);
        let run_id = store.save_warm_up_run(&new_run).unwrap();
        assert!(run_id > 0);

        let runs = store.list_warm_up_runs("gt3", 10).unwrap();
        assert_eq!(runs.len(), 1);

        let loaded = &runs[0];
        assert_eq!(loaded.id, run_id);
        assert_eq!(loaded.preset_id, new_run.preset_id);
        assert_eq!(loaded.started_at, new_run.started_at);
        assert!((loaded.score - expected_score).abs() < f32::EPSILON);
        assert_eq!(loaded.steps.len(), 3);
        assert_eq!(loaded.steps, new_run.steps);
        assert_eq!(loaded.steps[0].attempt_id, Some(saved_attempt_id));
    }

    #[test]
    fn list_warm_up_runs_ordering_filter_and_limit() {
        let store = AttemptStore::open_in_memory().unwrap();

        let make_run = |preset: &str, started_at: &str| NewWarmUpRun {
            preset_id: preset.to_string(),
            started_at: started_at.to_string(),
            steps: vec![WarmUpStepResult {
                drill_id: "drill-1".to_string(),
                reps: 1,
                skipped: false,
                attempt_id: None,
                score: Some(80.0),
            }],
        };

        let id1 = store
            .save_warm_up_run(&make_run("gt3", "2026-10-01T10:00:00Z"))
            .unwrap();
        let id2 = store
            .save_warm_up_run(&make_run("gt3", "2026-10-03T10:00:00Z"))
            .unwrap();
        let id3 = store
            .save_warm_up_run(&make_run("gt3", "2026-10-02T10:00:00Z"))
            .unwrap();
        let _id_mx5 = store
            .save_warm_up_run(&make_run("mx5", "2026-10-04T10:00:00Z"))
            .unwrap();

        // Newest first order
        let runs = store.list_warm_up_runs("gt3", 10).unwrap();
        assert_eq!(runs.len(), 3);
        assert_eq!(runs[0].id, id2);
        assert_eq!(runs[0].started_at, "2026-10-03T10:00:00Z");
        assert_eq!(runs[1].id, id3);
        assert_eq!(runs[1].started_at, "2026-10-02T10:00:00Z");
        assert_eq!(runs[2].id, id1);
        assert_eq!(runs[2].started_at, "2026-10-01T10:00:00Z");

        // Honours limit
        let limited = store.list_warm_up_runs("gt3", 2).unwrap();
        assert_eq!(limited.len(), 2);
        assert_eq!(limited[0].id, id2);
        assert_eq!(limited[1].id, id3);

        // Filters by preset
        let mx5_runs = store.list_warm_up_runs("mx5", 10).unwrap();
        assert_eq!(mx5_runs.len(), 1);
        assert_eq!(mx5_runs[0].started_at, "2026-10-04T10:00:00Z");

        let unknown_runs = store.list_warm_up_runs("unknown", 10).unwrap();
        assert_eq!(unknown_runs, []);
    }

    #[test]
    fn deleting_referenced_attempt_sets_step_attempt_id_to_none() {
        let store = AttemptStore::open_in_memory().unwrap();

        let attempt = NewAttempt {
            drill_id: "d1".to_string(),
            preset_id: "gt3".to_string(),
            pedal: Pedal::Brake,
            started_at: "2026-10-10T10:00:00Z".to_string(),
            aborted: false,
            best: Some(90.0),
            average: Some(90.0),
            consistency: None,
            reps: Vec::new(),
        };
        let attempt_id = store.save_attempt(&attempt).unwrap();

        let run = NewWarmUpRun {
            preset_id: "gt3".to_string(),
            started_at: "2026-10-10T10:30:00Z".to_string(),
            steps: vec![
                WarmUpStepResult {
                    drill_id: "d1".to_string(),
                    reps: 4,
                    skipped: false,
                    attempt_id: None,
                    score: Some(90.0),
                },
                WarmUpStepResult {
                    drill_id: "d2".to_string(),
                    reps: 6,
                    skipped: false,
                    attempt_id: Some(attempt_id),
                    score: Some(80.0),
                },
            ],
        };
        let run_id = store.save_warm_up_run(&run).unwrap();

        // Delete the attempt that step 1 references, using raw SQL
        store
            .raw_conn()
            .execute(
                "DELETE FROM attempt WHERE id = ?1;",
                rusqlite::params![attempt_id],
            )
            .unwrap();

        // The warm_up_run survives, but step 1's attempt_id is now None (ON DELETE SET NULL)
        let runs = store.list_warm_up_runs("gt3", 10).unwrap();
        assert_eq!(runs.len(), 1);
        let loaded = &runs[0];
        assert_eq!(loaded.id, run_id);
        assert_eq!(loaded.steps.len(), 2);
        assert_eq!(loaded.steps[0].attempt_id, None);
        assert_eq!(loaded.steps[0].drill_id, "d1");
        assert_eq!(loaded.steps[0].score, Some(90.0));
        assert_eq!(loaded.steps[1].attempt_id, None);
        assert_eq!(loaded.steps[1].drill_id, "d2");
        assert_eq!(loaded.steps[1].score, Some(80.0));
    }

    #[test]
    fn deleting_warm_up_run_cascades_to_steps() {
        let store = AttemptStore::open_in_memory().unwrap();

        let run = NewWarmUpRun {
            preset_id: "gt3".to_string(),
            started_at: "2026-10-10T13:00:00Z".to_string(),
            steps: vec![
                WarmUpStepResult {
                    drill_id: "d1".to_string(),
                    reps: 2,
                    skipped: false,
                    attempt_id: None,
                    score: Some(70.0),
                },
                WarmUpStepResult {
                    drill_id: "d2".to_string(),
                    reps: 2,
                    skipped: true,
                    attempt_id: None,
                    score: None,
                },
            ],
        };
        let run_id = store.save_warm_up_run(&run).unwrap();

        store
            .raw_conn()
            .execute(
                "DELETE FROM warm_up_run WHERE id = ?1;",
                rusqlite::params![run_id],
            )
            .unwrap();

        let step_count: i64 = store
            .raw_conn()
            .query_row(
                "SELECT COUNT(*) FROM warm_up_step WHERE run_id = ?1;",
                rusqlite::params![run_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(step_count, 0);
    }

    #[test]
    fn skipped_step_score_is_stored_as_none() {
        let store = AttemptStore::open_in_memory().unwrap();

        let run = NewWarmUpRun {
            preset_id: "gt3".to_string(),
            started_at: "2026-10-10T14:00:00Z".to_string(),
            steps: vec![WarmUpStepResult {
                drill_id: "d1".to_string(),
                reps: 3,
                skipped: true,
                attempt_id: None,
                score: Some(50.0),
            }],
        };
        store.save_warm_up_run(&run).unwrap();

        let runs = store.list_warm_up_runs("gt3", 10).unwrap();
        assert_eq!(runs[0].steps[0].score, None);
        assert!(runs[0].steps[0].skipped);
        assert_eq!(runs[0].score, 0.0);
    }

    #[test]
    fn invalid_warm_up_run_is_rejected_and_writes_nothing() {
        let store = AttemptStore::open_in_memory().unwrap();

        let no_steps = NewWarmUpRun {
            preset_id: "gt3".to_string(),
            started_at: "2026-10-10T15:00:00Z".to_string(),
            steps: vec![],
        };
        assert!(matches!(
            store.save_warm_up_run(&no_steps),
            Err(AttemptError::Invalid(_))
        ));

        let blank_preset = NewWarmUpRun {
            preset_id: "   ".to_string(),
            started_at: "2026-10-10T15:00:00Z".to_string(),
            steps: vec![WarmUpStepResult {
                drill_id: "d1".to_string(),
                reps: 1,
                skipped: false,
                attempt_id: None,
                score: Some(80.0),
            }],
        };
        let err = store.save_warm_up_run(&blank_preset).unwrap_err();
        assert!(matches!(err, AttemptError::Invalid(_)));
        assert!(std::error::Error::source(&err).is_none());

        let run_count: i64 = store
            .raw_conn()
            .query_row("SELECT COUNT(*) FROM warm_up_run;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(run_count, 0);
        let step_count: i64 = store
            .raw_conn()
            .query_row("SELECT COUNT(*) FROM warm_up_step;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(step_count, 0);
    }

    #[test]
    fn saving_nonexistent_attempt_id_fails_and_rolls_back() {
        let store = AttemptStore::open_in_memory().unwrap();

        let run = NewWarmUpRun {
            preset_id: "gt3".to_string(),
            started_at: "2026-10-10T12:00:00Z".to_string(),
            steps: vec![
                WarmUpStepResult {
                    drill_id: "d1".to_string(),
                    reps: 2,
                    skipped: false,
                    attempt_id: None,
                    score: Some(85.0),
                },
                WarmUpStepResult {
                    drill_id: "d2".to_string(),
                    reps: 2,
                    skipped: false,
                    attempt_id: Some(999_999), // does not exist
                    score: Some(90.0),
                },
            ],
        };

        let result = store.save_warm_up_run(&run);
        assert!(result.is_err(), "should fail due to foreign key constraint");

        // Verify rollback: no warm_up_run or warm_up_step rows remain
        let run_count: i64 = store
            .raw_conn()
            .query_row("SELECT COUNT(*) FROM warm_up_run;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(run_count, 0);

        let step_count: i64 = store
            .raw_conn()
            .query_row("SELECT COUNT(*) FROM warm_up_step;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(step_count, 0);

        let runs = store.list_warm_up_runs("gt3", 10).unwrap();
        assert_eq!(runs, []);
    }
}
