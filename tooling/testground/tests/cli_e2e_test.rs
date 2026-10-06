//! End-to-end tests for Evenframe CLI subcommands.
//!
//! These tests verify that:
//! 1. The CLI binary can be invoked
//! 2. Subcommands are properly registered
//! 3. Help output is correctly formatted
//! 4. Invalid commands produce appropriate errors

#[path = "support/binary.rs"]
mod binary;

use binary::evenframe;
use std::process::Command;

/// Helper to run evenframe with arguments
fn run_evenframe(args: &[&str]) -> std::process::Output {
    let binary = evenframe();
    Command::new(binary)
        .args(args)
        .output()
        .unwrap_or_else(|_| panic!("Failed to execute evenframe binary at {:?}", binary))
}

// ============================================================================
// Help and Version Tests
// ============================================================================

#[test]
fn test_cli_help() {
    let output = run_evenframe(&["--help"]);

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Should succeed
    assert!(
        output.status.success(),
        "evenframe --help should succeed. stderr: {}",
        stderr
    );

    // Should contain description
    assert!(
        stdout.contains("Evenframe") || stdout.contains("TypeScript"),
        "Help should mention Evenframe. stdout: {}",
        stdout
    );

    // Should list subcommands
    assert!(
        stdout.contains("typesync") || stdout.contains("Commands"),
        "Help should mention commands. stdout: {}",
        stdout
    );
}

#[test]
fn test_cli_version() {
    let output = run_evenframe(&["--version"]);

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should succeed
    assert!(
        output.status.success(),
        "evenframe --version should succeed"
    );

    // Should contain version number
    assert!(
        stdout.contains("evenframe") || stdout.contains("0."),
        "Version output should contain program name or version. stdout: {}",
        stdout
    );
}

// ============================================================================
// Subcommand Help Tests
// ============================================================================

#[test]
fn test_typesync_help() {
    let output = run_evenframe(&["typesync", "--help"]);

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success(), "typesync --help should succeed");

    // Should describe typesync functionality
    assert!(
        stdout.contains("type") || stdout.contains("generate") || stdout.contains("TypeScript"),
        "typesync help should mention types. stdout: {}",
        stdout
    );
}

#[test]
fn test_schemasync_help() {
    let output = run_evenframe(&["schemasync", "--help"]);

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success(), "schemasync --help should succeed");

    // Should describe schemasync functionality
    assert!(
        stdout.contains("schema") || stdout.contains("database") || stdout.contains("sync"),
        "schemasync help should mention schema/database. stdout: {}",
        stdout
    );
}

#[test]
fn test_generate_help() {
    let output = run_evenframe(&["generate", "--help"]);

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success(), "generate --help should succeed");

    // Should describe full pipeline
    assert!(
        stdout.contains("pipeline") || stdout.contains("typesync") || stdout.contains("schemasync"),
        "generate help should mention pipeline or both syncs. stdout: {}",
        stdout
    );
}

#[test]
fn test_init_help() {
    let output = run_evenframe(&["init", "--help"]);

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success(), "init --help should succeed");

    // Should describe init functionality
    assert!(
        stdout.contains("init") || stdout.contains("config") || stdout.contains("evenframe.toml"),
        "init help should mention initialization. stdout: {}",
        stdout
    );
}

#[test]
fn test_validate_help() {
    let output = run_evenframe(&["validate", "--help"]);

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success(), "validate --help should succeed");

    // Should describe validation
    assert!(
        stdout.contains("validate") || stdout.contains("config") || stdout.contains("types"),
        "validate help should mention validation. stdout: {}",
        stdout
    );
}

#[test]
fn test_info_help() {
    let output = run_evenframe(&["info", "--help"]);

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success(), "info --help should succeed");

    // Should describe info functionality
    assert!(
        stdout.contains("info") || stdout.contains("display") || stdout.contains("types"),
        "info help should mention information display. stdout: {}",
        stdout
    );
}

// ============================================================================
// Nested Subcommand Help Tests
// ============================================================================

#[test]
fn test_typesync_arktype_help() {
    let output = run_evenframe(&["typesync", "arktype", "--help"]);

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        output.status.success(),
        "typesync arktype --help should succeed"
    );

    assert!(
        stdout.contains("ArkType") || stdout.contains("arktype") || stdout.contains("output"),
        "arktype help should mention ArkType. stdout: {}",
        stdout
    );
}

#[test]
fn test_typesync_effect_help() {
    let output = run_evenframe(&["typesync", "effect", "--help"]);

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        output.status.success(),
        "typesync effect --help should succeed"
    );

    assert!(
        stdout.contains("Effect") || stdout.contains("effect") || stdout.contains("output"),
        "effect help should mention Effect. stdout: {}",
        stdout
    );
}

#[test]
fn test_typesync_flatbuffers_help() {
    let output = run_evenframe(&["typesync", "flatbuffers", "--help"]);

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        output.status.success(),
        "typesync flatbuffers --help should succeed"
    );

    assert!(
        stdout.contains("FlatBuffers") || stdout.contains("flatbuffers") || stdout.contains("fbs"),
        "flatbuffers help should mention FlatBuffers. stdout: {}",
        stdout
    );
}

#[test]
fn test_typesync_protobuf_help() {
    let output = run_evenframe(&["typesync", "protobuf", "--help"]);

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        output.status.success(),
        "typesync protobuf --help should succeed"
    );

    assert!(
        stdout.contains("Protocol") || stdout.contains("protobuf") || stdout.contains("proto"),
        "protobuf help should mention Protobuf. stdout: {}",
        stdout
    );
}

#[test]
fn test_schemasync_diff_help() {
    let output = run_evenframe(&["schemasync", "diff", "--help"]);

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        output.status.success(),
        "schemasync diff --help should succeed"
    );

    assert!(
        stdout.contains("diff") || stdout.contains("dry") || stdout.contains("format"),
        "diff help should mention diff functionality. stdout: {}",
        stdout
    );
}

#[test]
fn test_schemasync_apply_help() {
    let output = run_evenframe(&["schemasync", "apply", "--help"]);

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        output.status.success(),
        "schemasync apply --help should succeed"
    );

    assert!(
        stdout.contains("apply") || stdout.contains("changes") || stdout.contains("yes"),
        "apply help should mention applying changes. stdout: {}",
        stdout
    );
}

#[test]
fn test_schemasync_mock_help() {
    let output = run_evenframe(&["schemasync", "mock", "--help"]);

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        output.status.success(),
        "schemasync mock --help should succeed"
    );

    assert!(
        stdout.contains("mock") || stdout.contains("data") || stdout.contains("generate"),
        "mock help should mention mock data. stdout: {}",
        stdout
    );
}

// ============================================================================
// Global Flag Tests
// ============================================================================

#[test]
fn test_global_verbose_flag() {
    let output = run_evenframe(&["--help"]);

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("-v") || stdout.contains("verbose"),
        "Help should mention verbose flag. stdout: {}",
        stdout
    );
}

#[test]
fn test_global_quiet_flag() {
    let output = run_evenframe(&["--help"]);

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("-q") || stdout.contains("quiet"),
        "Help should mention quiet flag. stdout: {}",
        stdout
    );
}

#[test]
fn test_global_config_flag() {
    let output = run_evenframe(&["--help"]);

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("-c") || stdout.contains("config"),
        "Help should mention config flag. stdout: {}",
        stdout
    );
}

#[test]
fn test_global_output_flag() {
    let output = run_evenframe(&["--help"]);

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("-o") || stdout.contains("output"),
        "Help should mention output flag. stdout: {}",
        stdout
    );
}

#[test]
fn test_global_source_flag() {
    let output = run_evenframe(&["--help"]);

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("source") || stdout.contains("rust") || stdout.contains("flatbuffers"),
        "Help should mention source flag. stdout: {}",
        stdout
    );
}

// ============================================================================
// Error Handling Tests
// ============================================================================

#[test]
fn test_invalid_subcommand() {
    let output = run_evenframe(&["invalid-command"]);

    // Should fail
    assert!(!output.status.success(), "Invalid subcommand should fail");

    let stderr = String::from_utf8_lossy(&output.stderr);
    // Should mention the invalid command or suggest valid ones
    assert!(
        stderr.contains("invalid") || stderr.contains("error") || stderr.contains("unrecognized"),
        "Error should mention invalid command. stderr: {}",
        stderr
    );
}

#[test]
fn test_invalid_flag() {
    let output = run_evenframe(&["--invalid-flag"]);

    // Should fail
    assert!(!output.status.success(), "Invalid flag should fail");
}

// ============================================================================
// Combined Flag Tests
// ============================================================================

#[test]
fn test_multiple_verbose_levels() {
    // Test -v, -vv, -vvv
    for level in ["-v", "-vv", "-vvv"] {
        let output = run_evenframe(&[level, "--help"]);
        assert!(
            output.status.success(),
            "evenframe {} --help should succeed",
            level
        );
    }
}

#[test]
fn test_quiet_and_verbose_conflict() {
    // This might be allowed (quiet overrides) or might error
    // Just verify it doesn't crash
    let output = run_evenframe(&["-q", "-v", "--help"]);
    // Check it at least produces output
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stdout.is_empty() || !stderr.is_empty(),
        "Should produce some output"
    );
}
