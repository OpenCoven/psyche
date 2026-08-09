//! Process-abort helper for package-local SQLite atomicity tests.

use std::path::Path;

use psyche_store::migration_test_support::{
    MigrationFaultPoint, StoreFaultPoint, run_migration_with_fault, run_store_with_fault,
};

const EXIT_BEFORE_COMMIT: &str = "exit-before-commit";
const EXIT_AFTER_RECORD_BEFORE_TRANSITION: &str = "exit-after-record-before-transition";
const EXIT_AFTER_BINDING_REVISION_BEFORE_COMMIT: &str = "exit-after-binding-revision-before-commit";
const EXIT_AFTER_COMMIT_BEFORE_CHECKPOINT: &str = "exit-after-commit-before-checkpoint";
const EXIT_DURING_MIGRATION: &str = "exit-during-migration";

fn main() {
    let mut arguments = std::env::args_os().skip(1);
    let Some(mode) = arguments.next() else {
        fail("expected crash mode and database path");
    };
    let Some(database) = arguments.next() else {
        fail("expected crash mode and database path");
    };
    if arguments.next().is_some() {
        fail("expected crash mode and database path");
    }
    let Some(mode) = mode.to_str() else {
        fail("unknown crash mode");
    };

    let database = Path::new(&database);
    let result = match mode {
        EXIT_BEFORE_COMMIT => run_store_with_fault(database, StoreFaultPoint::BeforeCommit),
        EXIT_AFTER_RECORD_BEFORE_TRANSITION => {
            run_store_with_fault(database, StoreFaultPoint::AfterRecordBeforeTransition)
        }
        EXIT_AFTER_BINDING_REVISION_BEFORE_COMMIT => {
            run_store_with_fault(database, StoreFaultPoint::AfterBindingRevisionBeforeCommit)
        }
        EXIT_AFTER_COMMIT_BEFORE_CHECKPOINT => {
            run_store_with_fault(database, StoreFaultPoint::AfterCommitBeforeCheckpoint)
        }
        EXIT_DURING_MIGRATION => run_migration_with_fault(
            database,
            MigrationFaultPoint::AfterMigrationSqlBeforeUserVersion,
        ),
        _ => fail("unknown crash mode"),
    };
    if let Err(error) = result {
        fail(&error.to_string());
    }
}

fn fail(message: &str) -> ! {
    eprintln!("crash_writer: {message}");
    std::process::exit(2);
}
