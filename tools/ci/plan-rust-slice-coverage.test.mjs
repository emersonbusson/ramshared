import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { mkdtempSync, mkdirSync, readFileSync, rmSync, symlinkSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'

import {
  isCommentOnlyRustDifferential,
  main,
  runCoveragePlan,
  selectCoverageEntries,
  stripRustComments,
  validateCoverageMap,
} from './plan-rust-slice-coverage.mjs'

const REPOSITORY_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..')
const COMMENT_LANGUAGE_HIGH_COVERAGE_FILES = [
  'crates/ramshared-block/src/handshake.rs',
  'crates/ramshared-block/src/protocol.rs',
  'crates/ramshared-broker/src/arbiter.rs',
  'crates/ramshared-broker/src/protocol.rs',
  'crates/ramshared-wsl2d/src/broker_srv.rs',
  'crates/ramshared-wsl2d/src/canary_probe.rs',
  'crates/ramshared-wsl2d/src/telemetry.rs',
  'crates/ramshared-broker/src/model.rs',
  'crates/ramshared-broker/src/slices.rs',
  'crates/ramshared-wsl2d/src/residency.rs',
]
const COMMENT_LANGUAGE_BLOCKED_FILES = [
  'crates/ramshared-wsl2d/src/main.rs',
]
const COMMENT_LANGUAGE_FEATURE_OWNED_FILES = [
  'crates/ramshared-agent/src/main.rs',
  'crates/ramshared-cli/src/cascade/cascade_io.rs',
  'crates/ramshared-cli/src/main.rs',
  'crates/ramshared-wsl2d/src/conn.rs',
]
const WSL2_CONTROL_PLANE_COVERAGE_ENTRY = {
  id: 'wsl2-dual-plane-monitor',
  kind: 'rust-line-coverage',
  spec: 'docs/specs/no-milestone/wsl2-control-plane-pressure-incident/SPEC.md',
  command: [
    'node', 'tools/ci/check-rust-slice-coverage.mjs',
    '-p', 'ramshared-cli',
    '--files', 'crates/ramshared-cli/src/workload.rs,crates/ramshared-cli/src/supervisor.rs,crates/ramshared-cli/src/monitor.rs,crates/ramshared-cli/src/stress.rs',
    '--min', '80',
    '--report-json', 'tmp/wsl2-control-plane-pressure-incident-cov.json',
  ],
  packages: ['ramshared-cli'],
  files: [
    'crates/ramshared-cli/src/workload.rs',
    'crates/ramshared-cli/src/supervisor.rs',
    'crates/ramshared-cli/src/monitor.rs',
    'crates/ramshared-cli/src/stress.rs',
  ],
  min: 80,
}
const COMMENT_LANGUAGE_COVERAGE_ENTRY = {
  id: 'comment-language-rust-localization-high-coverage',
  kind: 'rust-line-coverage',
  spec: 'docs/specs/no-milestone/comment-language-integrity/SPEC.md',
  command: [
    'node', 'tools/ci/check-rust-slice-coverage.mjs',
    '-p', 'ramshared-block,ramshared-broker,ramshared-wsl2d',
    '--files', COMMENT_LANGUAGE_HIGH_COVERAGE_FILES.join(','),
    '--min', '80',
    '--report-json', 'tmp/comment-language-rust-cov.json',
  ],
  packages: ['ramshared-block', 'ramshared-broker', 'ramshared-wsl2d'],
  files: COMMENT_LANGUAGE_HIGH_COVERAGE_FILES,
  min: 80,
}
const MICROSOFT_NATIVE_VRAM_N3_COVERAGE_ENTRY = {
  id: 'microsoft-native-vram-memory-tier-n3-state',
  kind: 'rust-line-coverage',
  spec: 'docs/specs/no-milestone/microsoft-native-vram-memory-tier/SPEC.md',
  command: [
    'node', 'tools/ci/check-rust-slice-coverage.mjs',
    '-p', 'ramshared-tier',
    '--files', 'crates/ramshared-tier/src/n3_state.rs',
    '--min', '80',
    '--report-json', 'tmp/microsoft-native-vram-memory-tier-n3-cov.json',
  ],
  packages: ['ramshared-tier'],
  files: ['crates/ramshared-tier/src/n3_state.rs'],
  min: 80,
}
const MICROSOFT_NATIVE_VRAM_N3_MODULE_EXPORT_GLUE_ENTRY = {
  id: 'microsoft-native-vram-memory-tier-n3-module-export-glue',
  kind: 'rust-module-export-glue-differential',
  spec: 'docs/specs/no-milestone/microsoft-native-vram-memory-tier/SPEC.md',
  files: ['crates/ramshared-tier/src/lib.rs'],
  package: 'ramshared-tier',
  declaration: 'pub mod n3_state;\npub mod nbd_readiness;',
  cargo_test: ['cargo', 'test', '-p', 'ramshared-tier', '--all-targets'],
}
const WSL2_NBD_PRODUCT_READINESS_COVERAGE_ENTRY = {
  id: 'wsl2-nbd-product-readiness',
  kind: 'rust-line-coverage',
  spec: 'docs/specs/no-milestone/wsl2-nbd-product-readiness/SPEC.md',
  command: [
    'node', 'tools/ci/check-rust-slice-coverage.mjs',
    '-p', 'ramshared-tier',
    '--files', 'crates/ramshared-tier/src/nbd_readiness.rs',
    '--min', '80',
    '--report-json', 'tmp/wsl2-nbd-product-readiness-cov.json',
  ],
  packages: ['ramshared-tier'],
  files: ['crates/ramshared-tier/src/nbd_readiness.rs'],
  min: 80,
}
const WSL2_NBD_PRODUCT_READINESS_TESTS = [
  'nbd_only_transport_is_the_only_ready_value',
  'lower_tier_formula_uses_ten_percent_or_512_mib',
  'capacity_shortfall_refuses_before_mutation',
  'product_off_is_not_ready_alias',
  'deterministic_gate_failure_is_not_retried',
  'activation_and_deactivation_are_idempotent',
]
const COMMENT_LANGUAGE_TEST_ONLY_ENTRY = {
  id: 'comment-language-rust-test-only-localization',
  kind: 'rust-test-only-localization-differential',
  spec: 'docs/specs/no-milestone/comment-language-integrity/SPEC.md',
  files: [
    'crates/ramshared-cuda/src/lib.rs',
  ],
  verifications: [
    {
      source: 'crates/ramshared-cuda/src/lib.rs',
      package: 'ramshared-cuda',
      test_module: 'tests',
      cargo_test: ['cargo', 'test', '-p', 'ramshared-cuda', '--lib'],
      ignored_gpu_tests: [{
        name: 'gpu_roundtrip_256mib',
        command: ['cargo', 'test', '-p', 'ramshared-cuda', '--', '--ignored', '--test-threads=1'],
        evidence: 'validation.md',
      }],
    },
  ],
}
const MEMORY_BROKER_WSL2D_BACKEND_COVERAGE_ENTRY = {
  id: 'memory-broker-wsl2d-backend',
  kind: 'rust-line-coverage',
  spec: 'docs/specs/no-milestone/memory-broker/SPEC.md',
  command: [
    'node', 'tools/ci/check-rust-slice-coverage.mjs',
    '-p', 'ramshared-wsl2d',
    '--files', 'crates/ramshared-wsl2d/src/backend.rs',
    '--min', '80',
    '--report-json', 'tmp/memory-broker-wsl2d-backend-cov.json',
  ],
  packages: ['ramshared-wsl2d'],
  files: ['crates/ramshared-wsl2d/src/backend.rs'],
  min: 80,
}
const MEMORY_BROKER_WSL2D_BACKEND_GPU_RELOCATION_ENTRY = {
  id: 'memory-broker-wsl2d-backend-gpu-test-relocation',
  kind: 'rust-ignored-test-relocation',
  spec: 'docs/specs/no-milestone/memory-broker/SPEC.md',
  files: ['crates/ramshared-wsl2d/src/backend.rs'],
  base_revision: '69f7469fa999b7d079341ee6bf8ebb006d517b51',
  base_source_sha256: 'b58d99366164b7e898baa42492fead82416a6169ec06ce16fb274b06b6d99663',
  verification: {
    source: 'crates/ramshared-wsl2d/src/backend.rs',
    package: 'ramshared-wsl2d',
    test_module: 'tests',
    ignored_test_source: 'crates/ramshared-wsl2d/tests/backend_gpu.rs',
    ignored_test_target: 'backend_gpu',
    relocation_imports: {
      base: [
        'use super::*;',
        'use ramshared_block::{Command, Request, serve};',
        'use ramshared_cuda::Cuda;',
      ],
      head: [
        'use ramshared_block::{Command, Request, serve};',
        'use ramshared_cuda::Cuda;',
        'use ramshared_wsl2d::VramBackend;',
      ],
    },
    ignored_gpu_tests: [
      {
        name: 'vram_backend_serves_nbd_write_then_read',
        command: [
          'cargo', 'test', '-p', 'ramshared-wsl2d', '--test', 'backend_gpu',
          'vram_backend_serves_nbd_write_then_read', '--', '--ignored', '--test-threads=1',
        ],
        historical_command: [
          'cargo', 'test', '-p', 'ramshared-wsl2d',
          'backend::tests::vram_backend_serves_nbd_write_then_read',
          '--', '--ignored', '--test-threads=1',
        ],
        evidence: 'validation.md',
      },
      {
        name: 'vram_gauge_outros_captures_real_graphics_usage',
        command: [
          'cargo', 'test', '-p', 'ramshared-wsl2d', '--test', 'backend_gpu',
          'vram_gauge_outros_captures_real_graphics_usage', '--', '--ignored', '--test-threads=1',
        ],
        historical_command: [
          'cargo', 'test', '-p', 'ramshared-wsl2d',
          'backend::tests::vram_gauge_outros_captures_real_graphics_usage',
          '--', '--ignored', '--test-threads=1',
        ],
        evidence: 'validation.md',
      },
    ],
  },
}
const MEMORY_BROKER_AGENT_CLI_COVERAGE_ENTRY = {
  id: 'memory-broker-agent-cli',
  kind: 'rust-line-coverage',
  spec: 'docs/specs/no-milestone/memory-broker/SPEC.md',
  command: [
    'node', 'tools/ci/check-rust-slice-coverage.mjs',
    '-p', 'ramshared-agent',
    '--files', 'crates/ramshared-agent/src/main.rs',
    '--min', '80',
    '--report-json', 'tmp/memory-broker-agent-cli-cov.json',
  ],
  packages: ['ramshared-agent'],
  files: ['crates/ramshared-agent/src/main.rs'],
  min: 80,
}
const MEMORY_BROKER_AGENT_CLI_PROCESS_TESTS = [
  'cli_help_prints_usage_and_exits_zero',
  'cli_missing_broker_refuses_with_exit_two',
  'cli_invalid_transport_refuses_with_exit_two',
  'cli_missing_tenant_refuses_with_exit_two',
  'cli_status_reply_prints_public_status_and_exits_zero',
  'cli_status_broker_refusal_exits_one',
  'cli_status_timeout_exits_one_within_six_seconds',
]
const MEMORY_BROKER_AGENT_CLI_MAIN_TESTS = [
  'help_is_a_parse_outcome',
  'usage_diagnostic_adds_usage_once',
  'swap_on_prefers_broker_priority_without_running_swap',
  'demote_all_dispatches_release_without_running_swap',
  'session_registers_dispatches_commands_and_stops_on_refusal',
  'session_reports_execution_results_without_running_swap',
  'session_watchdog_terminates_silent_broker_without_swap',
]
const MEMORY_BROKER_WSL2D_DAEMON_COVERAGE_ENTRY = {
  id: 'memory-broker-wsl2d-daemon',
  kind: 'rust-line-coverage',
  spec: 'docs/specs/no-milestone/memory-broker/SPEC.md',
  command: [
    'node', 'tools/ci/check-rust-slice-coverage.mjs',
    '-p', 'ramshared-wsl2d',
    '--files', 'crates/ramshared-wsl2d/src/main.rs,crates/ramshared-wsl2d/src/swap.rs',
    '--min', '80',
    '--report-json', 'tmp/memory-broker-wsl2d-daemon-cov.json',
  ],
  packages: ['ramshared-wsl2d'],
  files: [
    'crates/ramshared-wsl2d/src/main.rs',
    'crates/ramshared-wsl2d/src/swap.rs',
  ],
  min: 80,
}
const MEMORY_BROKER_WSL2D_DAEMON_MAIN_TESTS = [
  'daemon_args_accept_broker_wiring_and_normalize_addresses',
  'daemon_args_refuse_invalid_or_unsafe_combinations_before_backend',
  'daemon_args_cover_flag_boundaries_before_backend',
  'daemon_broker_config_preserves_telemetry_and_exact_endpoints',
  'daemon_broker_ram_binds_loopback_and_cleans_owned_socket',
  'daemon_broker_setup_stops_bounded_without_backend',
  'daemon_broker_acceptor_failure_rolls_back_owned_socket',
  'daemon_broker_vram_and_ram_lifecycles_use_injected_runtime',
  'daemon_broker_setup_failure_zeroes_allocated_vram_before_return',
  'daemon_broker_lock_refusal_zeroes_allocated_vram_before_return',
  'daemon_broker_bind_conflict_refuses_and_preserves_existing_listener',
  'daemon_broker_panic_propagates_after_bounded_worker_cleanup',
  'daemon_worker_reply_is_io_accounting_barrier_and_shutdown_is_bounded',
  'daemon_worker_shutdown_wake_is_not_timer_dependent',
  'daemon_worker_shutdown_full_queue_is_nonblocking',
  'daemon_worker_parallel_full_queue_shutdowns_reap_without_notifier_threads',
  'daemon_worker_shutdown_preempts_queued_io_at_iteration_boundary',
  'daemon_worker_terminal_flag_wins_over_512_continuous_queue_refills',
  'daemon_command_timeout_terminates_child_without_hang',
  'daemon_nbd_serves_two_connection_generations_before_explicit_shutdown',
  'daemon_nbd_sparse_floor_refusal_reclaims_without_provider_allocation',
  'daemon_nbd_budget_poll_uses_injected_wddm_snapshot_and_global_probe',
  'daemon_nbd_budget_constraint_demotes_then_recovers_with_fake_swap',
  'daemon_nbd_recovery_activation_does_not_block_nbd_jobs',
  'daemon_nbd_recovery_failure_parks_without_relaunch',
  'daemon_nbd_shutdown_with_pending_recovery_fails_closed',
  'daemon_nbd_teardown_refuses_until_fake_usage_and_swapoff_confirm',
  'daemon_nbd_residency_demote_uses_injected_clock_and_swapoff',
  'daemon_ublk_runtime_orders_lifecycle_and_rolls_back_without_device',
  'daemon_ublk_runtime_failures_delete_only_after_fresh_absence_proof',
  'daemon_ublk_vulkan_refuses_before_device_mutation',
  'daemon_production_runner_refuses_safe_terminal_actions_before_platform_load',
  'daemon_ublk_wsl_guard_and_memory_lock_policy_are_pure_and_fail_closed',
]
const MEMORY_BROKER_WSL2D_DAEMON_PROCESS_TESTS = [
  'daemon_process_refusals_exit_before_backend',
]
const WSL2_CONN_COVERAGE_ENTRY = {
  id: 'wsl2-cascade-connection-transport',
  kind: 'rust-line-coverage',
  spec: 'docs/specs/no-milestone/wsl2-cascade-swap/SPEC.md',
  command: [
    'node', 'tools/ci/check-rust-slice-coverage.mjs',
    '-p', 'ramshared-wsl2d',
    '--files', 'crates/ramshared-wsl2d/src/conn.rs',
    '--min', '80',
    '--report-json', 'tmp/wsl2-conn-cov.json',
  ],
  packages: ['ramshared-wsl2d'],
  files: ['crates/ramshared-wsl2d/src/conn.rs'],
  min: 80,
}
const CASCADE_LIFECYCLE_CLI_COVERAGE_ENTRY = {
  id: 'cascade-lifecycle-observability',
  kind: 'rust-line-coverage',
  spec: 'docs/specs/no-milestone/cascade-lifecycle-observability/SPEC.md',
  command: [
    'node', 'tools/ci/check-rust-slice-coverage.mjs',
    '-p', 'ramshared-cli',
    '--files', 'crates/ramshared-cli/src/cascade/lifecycle.rs,crates/ramshared-cli/src/cascade/mod.rs,crates/ramshared-cli/src/main.rs,crates/ramshared-cli/src/diagnose.rs',
    '--min', '80',
    '--report-json', 'tmp/cascade-lifecycle-cov.json',
  ],
  packages: ['ramshared-cli'],
  files: [
    'crates/ramshared-cli/src/cascade/lifecycle.rs',
    'crates/ramshared-cli/src/cascade/mod.rs',
    'crates/ramshared-cli/src/main.rs',
    'crates/ramshared-cli/src/diagnose.rs',
  ],
  min: 80,
}
const CASCADE_LIFECYCLE_CLI_MAIN_TESTS = [
  'dispatch_refuses_invalid_flags_before_action',
  'dispatch_forwards_exact_status_and_diagnose_args',
]
const CASCADE_LIFECYCLE_CLI_PROCESS_TESTS = [
  'cli_help_and_unknown_command',
  'cli_check_and_doctor_report_decision_json_and_text',
  'cli_status_flags_are_exact_and_read_only',
  'cli_diagnose_forwards_event_args',
  'cli_up_and_down_refuse_before_mutation',
]
const CASCADE_TRANSPORT_ORCHESTRATION_COVERAGE_ENTRY = {
  id: 'cascade-transport-orchestration',
  kind: 'rust-line-coverage',
  spec: 'docs/specs/no-milestone/cascade-transport-policy/SPEC.md',
  command: [
    'node', 'tools/ci/check-rust-slice-coverage.mjs',
    '-p', 'ramshared-cli',
    '--files', 'crates/ramshared-cli/src/bounded_process.rs,crates/ramshared-cli/src/cascade/cascade_io.rs',
    '--min', '80',
    '--report-json', 'tmp/cascade-transport-orchestration-cov.json',
  ],
  packages: ['ramshared-cli'],
  files: [
    'crates/ramshared-cli/src/bounded_process.rs',
    'crates/ramshared-cli/src/cascade/cascade_io.rs',
  ],
  min: 80,
}
const CASCADE_TRANSPORT_ORCHESTRATION_CASCADE_TESTS = [
  'bounded_command_captures_stdout_and_rejects_nonzero',
  'bounded_command_times_out_and_reaps_its_direct_child',
  'bounded_command_contains_descendant_that_inherits_output',
  'zram_output_requires_exact_block_identity',
  'daemon_pid_requires_positive_pid_and_exact_identity',
  'failed_readiness_terminates_only_spawned_child',
  'connect_nbd_preserves_primary_error_and_rolls_back_once',
  'connect_nbd_refusal_terminates_exact_daemon_without_detach',
  'connect_nbd_uncertain_swapon_preserves_backend_and_daemon',
  'zram_fallback_refuses_unexpected_device_without_swapon',
  'malformed_zram_success_resets_exact_new_device_without_leak',
  'zram_setup_never_mutates_unbound_sysfs_fallback',
  'runtime_marker_and_pid_record_refuse_unsafe_identity',
  'setup_new_cascade_uses_only_temp_runtime_and_direct_child_fixture',
  'setup_new_cascade_rolls_back_zram_after_nbd_failure',
  'setup_new_cascade_keeps_zram_record_on_swapoff_refusal',
  'down_with_runtime_preserves_swapoff_first_and_cleans_temp_state',
  'transport_refusal_is_fail_closed_before_command',
]
const CASCADE_TRANSPORT_ORCHESTRATION_RUNNER_TESTS = [
  'capture_runner_keeps_legitimate_success_and_nonzero_status_typed',
  'capture_runner_rejects_bounded_output_overflow',
  'unreaped_group_selects_fatal_controller_containment',
]

function fixtureRoot(specText) {
  const root = mkdtempSync(path.join(tmpdir(), 'ramshared-rust-coverage-'))
  mkdirSync(path.join(root, 'docs', 'specs', 'no-milestone', 'fixture'), { recursive: true })
  mkdirSync(path.join(root, 'crates', 'fixture', 'src'), { recursive: true })
  writeFileSync(path.join(root, 'docs', 'specs', 'no-milestone', 'fixture', 'SPEC.md'), specText)
  writeFileSync(path.join(root, 'crates', 'fixture', 'src', 'policy.rs'), 'pub fn policy() {}\n')
  return root
}

function moduleExportGlueRoot(
  entry = MICROSOFT_NATIVE_VRAM_N3_MODULE_EXPORT_GLUE_ENTRY,
  headSource = 'pub mod cascade;\npub mod priority;\npub mod n3_state;\npub mod nbd_readiness;\n',
) {
  const root = mkdtempSync(path.join(tmpdir(), 'ramshared-module-export-'))
  mkdirSync(path.join(root, 'docs', 'specs', 'no-milestone', 'microsoft-native-vram-memory-tier'), { recursive: true })
  mkdirSync(path.join(root, 'crates', 'ramshared-tier', 'src'), { recursive: true })
  writeFileSync(
    path.join(root, 'docs', 'specs', 'no-milestone', 'microsoft-native-vram-memory-tier', 'SPEC.md'),
    embeddedOwnership('rust-slice-module-export-glue-differential-v1', {
      schema_version: 1,
      id: entry.id,
      kind: entry.kind,
      files: entry.files,
      package: entry.package,
      declaration: entry.declaration,
      cargo_test: entry.cargo_test,
    }),
  )
  writeFileSync(path.join(root, entry.files[0]), headSource)
  return root
}

function moduleExportGlueBaseReader(source = 'pub mod cascade;\npub mod priority;\n') {
  return (_revision, file) => file === 'crates/ramshared-tier/src/lib.rs' ? source : null
}

function coverageMap() {
  return {
    schema_version: 2,
    entries: [{
      id: 'fixture-policy',
      kind: 'rust-line-coverage',
      spec: 'docs/specs/no-milestone/fixture/SPEC.md',
      command: [
        'node', 'tools/ci/check-rust-slice-coverage.mjs', '-p', 'fixture',
        '--files', 'crates/fixture/src/policy.rs', '--min', '80',
      ],
      packages: ['fixture'],
      files: ['crates/fixture/src/policy.rs'],
      min: 80,
    }],
  }
}

function writeFixtureFile(root, relativePath, text) {
  const target = path.join(root, relativePath)
  mkdirSync(path.dirname(target), { recursive: true })
  writeFileSync(target, text)
}

function embeddedOwnership(marker, declaration) {
  return `<!-- ${marker}\n${JSON.stringify(declaration, null, 2)}\n-->\n`
}

function platformEntry() {
  return {
    id: 'fixture-platform',
    kind: 'windows-platform-e2e',
    spec: 'docs/specs/no-milestone/fixture/SPEC.md',
    files: ['crates/fixture/src/platform.rs'],
    verifications: [{
      source: 'crates/fixture/src/platform.rs',
      static: {
        path: 'scripts/windows/Test-PlatformStatic.ps1',
        test: 'platform_static_contract',
      },
      live: {
        path: 'scripts/windows/Run-Platform.ps1',
        test: 'platform_live_contract',
      },
    }],
  }
}

function platformDeclaration(entry) {
  return {
    schema_version: 1,
    id: entry.id,
    kind: entry.kind,
    files: entry.files,
    verifications: entry.verifications,
  }
}

function platformRoot(entry = platformEntry(), { wrapperIncludesStatic = true } = {}) {
  const specText = embeddedOwnership('rust-slice-platform-e2e-v1', platformDeclaration(entry))
  const root = fixtureRoot(specText)
  writeFixtureFile(root, entry.files[0], 'pub fn platform() {}\n')
  writeFixtureFile(root, entry.verifications[0].static.path,
    'Assert-Static $true "platform_static_contract" "fixture"\n')
  writeFixtureFile(root, entry.verifications[0].live.path,
    'Pass "platform_live_contract" "fixture"\n')
  writeFixtureFile(root, 'scripts/windows/Test-WindowsCiStatic.ps1',
    wrapperIncludesStatic ? '@{ Name = "Test-PlatformStatic.ps1" }\n' : '@{ Name = "Test-OtherStatic.ps1" }\n')
  return root
}

function structuralEntry() {
  return {
    id: 'fixture-structural',
    kind: 'rust-structural-contract',
    spec: 'docs/specs/no-milestone/fixture/SPEC.md',
    files: ['crates/fixture/src/lib.rs'],
    verifications: [{
      source: 'crates/fixture/src/lib.rs',
      package: 'fixture',
      cargo_test: ['cargo', 'test', '-p', 'fixture', '--lib'],
    }],
  }
}

function structuralDeclaration(entry) {
  return {
    schema_version: 1,
    id: entry.id,
    kind: entry.kind,
    files: entry.files,
    verifications: entry.verifications,
  }
}

function structuralRoot(entry = structuralEntry(), source = `#![forbid(unsafe_code)]

pub mod policy;
#[cfg(windows)]
pub mod windows;
pub use policy::{Decision, decide};
pub(crate) use policy as internal_policy;
`) {
  const specText = embeddedOwnership('rust-slice-structural-contract-v1', structuralDeclaration(entry))
  const root = fixtureRoot(specText)
  writeFixtureFile(root, entry.files[0], source)
  return root
}

function localizationEntry() {
  return {
    id: 'fixture-localization',
    kind: 'rust-localization-comment-differential',
    spec: 'docs/specs/no-milestone/fixture/SPEC.md',
    files: ['crates/fixture/src/localized.rs'],
  }
}

function localizationDeclaration(entry) {
  return {
    schema_version: 1,
    id: entry.id,
    kind: entry.kind,
    files: entry.files,
  }
}

function localizationRoot(entry = localizationEntry(), headSource = '// English comment\npub fn policy() {}\n') {
  const specText = embeddedOwnership('rust-slice-localization-comment-differential-v1', localizationDeclaration(entry))
  const root = fixtureRoot(specText)
  writeFixtureFile(root, entry.files[0], headSource)
  return root
}

function testOnlyLocalizationEntry() {
  return {
    id: 'fixture-test-only-localization',
    kind: 'rust-test-only-localization-differential',
    spec: 'docs/specs/no-milestone/fixture/SPEC.md',
    files: ['crates/fixture/src/test_only.rs'],
    verifications: [{
      source: 'crates/fixture/src/test_only.rs',
      package: 'fixture',
      test_module: 'tests',
      cargo_test: ['cargo', 'test', '-p', 'fixture', '--lib'],
      ignored_gpu_tests: [{
        name: 'gpu_roundtrip',
        command: ['cargo', 'test', '-p', 'fixture', 'gpu_roundtrip', '--', '--ignored', '--test-threads=1'],
        evidence: 'validation.md',
      }],
    }],
  }
}

function testOnlyLocalizationDeclaration(entry) {
  return {
    schema_version: 1,
    id: entry.id,
    kind: entry.kind,
    files: entry.files,
    verifications: entry.verifications,
  }
}

const TEST_ONLY_BASE_SOURCE = `// Portuguese production comment
const PROD: &str = r#"stable { #[cfg(test)] }"#;
const C_RAW: &str = cr#"stable " // literal text"#;
const BYTE: u8 = b'{';
struct Borrowed<'a>(&'a str);

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "requires GPU"]
    fn gpu_roundtrip() {
        assert_eq!("Portuguese test text", "Portuguese test text");
    }
}
`

const TEST_ONLY_HEAD_SOURCE = `// English production comment
const PROD: &str = r#"stable { #[cfg(test)] }"#;
const C_RAW: &str = cr#"stable " // literal text"#;
const BYTE: u8 = b'{';
struct Borrowed<'a>(&'a str);

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "requires GPU"]
    fn gpu_roundtrip() {
        assert_eq!("English test text", "English test text");
    }
}
`

function testOnlyEvidence(entry) {
  return entry.verifications.flatMap((verification) => verification.ignored_gpu_tests)
    .map((ignored) => `- \`${(ignored.historical_command ?? ignored.command).join(' ')}\`: **PASS**.`)
    .join('\n')
}

function testOnlyLocalizationRoot(entry = testOnlyLocalizationEntry(), headSource = TEST_ONLY_HEAD_SOURCE) {
  const specText = embeddedOwnership(
    'rust-slice-test-only-localization-differential-v1',
    testOnlyLocalizationDeclaration(entry),
  )
  const root = fixtureRoot(specText)
  writeFixtureFile(root, entry.files[0], headSource)
  writeFixtureFile(root, 'validation.md', testOnlyEvidence(entry))
  return root
}

function testOnlyBaseReader(entry, source = TEST_ONLY_BASE_SOURCE, evidence = testOnlyEvidence(entry)) {
  return (_revision, file) => {
    if (file === entry.files[0]) return source
    if (file === 'validation.md') return evidence
    return null
  }
}

function relocatedTestOnlyLocalizationEntry() {
  return {
    id: 'fixture-ignored-test-relocation',
    kind: 'rust-ignored-test-relocation',
    spec: 'docs/specs/no-milestone/fixture/SPEC.md',
    files: ['crates/fixture/src/test_only.rs'],
    base_revision: 'd'.repeat(40),
    base_source_sha256: createHash('sha256').update(RELOCATED_TEST_ONLY_BASE_SOURCE).digest('hex'),
    verification: {
      source: 'crates/fixture/src/test_only.rs',
      package: 'fixture',
      test_module: 'tests',
      ignored_test_source: 'crates/fixture/tests/backend_gpu.rs',
      ignored_test_target: 'backend_gpu',
      relocation_imports: {
        base: ['use super::*;'],
        head: ['use fixture::Gpu;'],
      },
      ignored_gpu_tests: [{
        name: 'gpu_roundtrip',
        command: [
          'cargo', 'test', '-p', 'fixture', '--test', 'backend_gpu', 'gpu_roundtrip',
          '--', '--ignored', '--test-threads=1',
        ],
        historical_command: [
          'cargo', 'test', '-p', 'fixture', 'test_only::tests::gpu_roundtrip',
          '--', '--ignored', '--test-threads=1',
        ],
        evidence: 'validation.md',
      }],
    },
  }
}

const RELOCATED_TEST_ONLY_BASE_SOURCE = `const PROD: &str = "stable";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires GPU"]
    fn gpu_roundtrip() {
        use crate::telemetry::Gauge;
        let payload = 7u64;
        assert_eq!(payload, 7u64, "stable message: {payload}");
        eprintln!("stable log: {payload}");
    }
}
`

const RELOCATED_TEST_ONLY_HEAD_SOURCE = `const PROD: &str = "stable";

#[cfg(test)]
mod tests {
}
`

const RELOCATED_TEST_ONLY_INTEGRATION_SOURCE = `use fixture::Gpu;

#[test]
#[ignore = "requires GPU"]
fn gpu_roundtrip() {
    use fixture::telemetry::Gauge;
    let payload = 7u64;
    assert_eq!(payload, 7u64, "stable message: {payload}");
    eprintln!("stable log: {payload}");
}
`

function relocatedTestOnlyLocalizationRoot(
  entry = relocatedTestOnlyLocalizationEntry(),
  integrationSource = RELOCATED_TEST_ONLY_INTEGRATION_SOURCE,
) {
  const root = fixtureRoot(embeddedOwnership('rust-slice-ignored-test-relocation-v1', relocationDeclaration(entry)))
  writeFixtureFile(root, entry.files[0], RELOCATED_TEST_ONLY_HEAD_SOURCE)
  writeFixtureFile(root, entry.verification.ignored_test_source, integrationSource)
  return root
}

function relocationDeclaration(entry) {
  return {
    schema_version: 1,
    id: entry.id,
    kind: entry.kind,
    files: entry.files,
    base_revision: entry.base_revision,
    base_source_sha256: entry.base_source_sha256,
    verification: entry.verification,
  }
}

function relocationVariant({
  id,
  spec,
  source = 'crates/fixture/src/test_only.rs',
  target = 'backend_gpu',
  name = 'gpu_roundtrip',
} = {}) {
  const entry = structuredClone(relocatedTestOnlyLocalizationEntry())
  if (id) entry.id = id
  if (spec) entry.spec = spec
  entry.files = [source]
  entry.verification.source = source
  entry.verification.ignored_test_target = target
  entry.verification.ignored_test_source = `crates/fixture/tests/${target}.rs`
  entry.verification.ignored_gpu_tests = [{
    name,
    command: [
      'cargo', 'test', '-p', 'fixture', '--test', target, name,
      '--', '--ignored', '--test-threads=1',
    ],
    historical_command: [
      'cargo', 'test', '-p', 'fixture', `${path.posix.basename(source, '.rs')}::tests::${name}`,
      '--', '--ignored', '--test-threads=1',
    ],
    evidence: 'validation.md',
  }]
  return entry
}

function relocationIntegrationSource(name = 'gpu_roundtrip') {
  return RELOCATED_TEST_ONLY_INTEGRATION_SOURCE.replaceAll('gpu_roundtrip', name)
}

function relocationOwnershipRoot(entries) {
  const root = mkdtempSync(path.join(tmpdir(), 'ramshared-rust-relocation-'))
  for (const entry of entries) {
    writeFixtureFile(root, entry.spec, embeddedOwnership(
      'rust-slice-ignored-test-relocation-v1',
      relocationDeclaration(entry),
    ))
    writeFixtureFile(root, entry.files[0], RELOCATED_TEST_ONLY_HEAD_SOURCE)
    const name = entry.verification.ignored_gpu_tests[0].name
    if (!readFileIfPresent(root, entry.verification.ignored_test_source)) {
      writeFixtureFile(root, entry.verification.ignored_test_source, relocationIntegrationSource(name))
    }
  }
  return root
}

function readFileIfPresent(root, relativePath) {
  try {
    return readFileSync(path.join(root, relativePath))
  } catch {
    return null
  }
}

function lineOwnerFor(source, id, spec) {
  return {
    id,
    kind: 'rust-line-coverage',
    spec,
    command: [
      'node', 'tools/ci/check-rust-slice-coverage.mjs', '-p', 'fixture',
      '--files', source, '--min', '80',
    ],
    packages: ['fixture'],
    files: [source],
    min: 80,
  }
}

function relocatedTestOnlyBaseReader(
  entry,
  source = RELOCATED_TEST_ONLY_BASE_SOURCE,
  evidence = entry.verification.ignored_gpu_tests
    .map((ignored) => `- \`${ignored.historical_command.join(' ')}\`: **PASS**.`)
    .join('\n'),
) {
  return (_revision, file) => {
    if (file === entry.files[0]) return source
    if (file === 'validation.md') return evidence
    return null
  }
}

function plannerRoot(specText = '```bash\nnode tools/ci/check-rust-slice-coverage.mjs -p fixture --files crates/fixture/src/policy.rs --min 80\n```\n') {
  const root = fixtureRoot(specText)
  mkdirSync(path.join(root, 'docs', 'governance'), { recursive: true })
  writeFileSync(path.join(root, 'docs', 'governance', 'rust-slice-coverage.json'), `${JSON.stringify(coverageMap())}\n`)
  writeFileSync(path.join(root, 'changed.txt'), 'crates/fixture/src/policy.rs\n')
  return root
}

test('spec_coverage_map_requires_exact_command_in_spec', () => {
  const root = fixtureRoot('```bash\nnode tools/ci/check-rust-slice-coverage.mjs -p fixture --files crates/fixture/src/other.rs --min 80\n```\n')
  const result = validateCoverageMap(coverageMap(), root)
  assert.equal(result.ok, false)
  assert.equal(result.errors.some((item) => item.rule === 'spec-command-missing'), true)
})

test('changed_business_rust_file_requires_mapped_spec_command', () => {
  const root = fixtureRoot('```bash\nnode tools/ci/check-rust-slice-coverage.mjs -p fixture --files crates/fixture/src/policy.rs --min 80\n```\n')
  const mapped = selectCoverageEntries(coverageMap(), ['crates/fixture/src/policy.rs'], root)
  assert.equal(mapped.ok, true)
  assert.deepEqual(mapped.entries.map((entry) => entry.id), ['fixture-policy'])

  const unmapped = selectCoverageEntries(coverageMap(), ['crates/fixture/src/unmapped.rs'], root)
  assert.equal(unmapped.ok, false)
  assert.equal(unmapped.errors.some((item) => item.rule === 'changed-rust-file-unmapped'), true)
})

test('microsoft_native_vram_n3_state_has_exact_coverage_owner', () => {
  const map = JSON.parse(readFileSync(path.join(REPOSITORY_ROOT, 'docs', 'governance', 'rust-slice-coverage.json'), 'utf8'))
  const entry = map.entries.find((item) => item.id === MICROSOFT_NATIVE_VRAM_N3_COVERAGE_ENTRY.id)
  assert.deepEqual(entry, MICROSOFT_NATIVE_VRAM_N3_COVERAGE_ENTRY)

  const selected = selectCoverageEntries(
    { schema_version: 2, entries: [entry] },
    MICROSOFT_NATIVE_VRAM_N3_COVERAGE_ENTRY.files,
    REPOSITORY_ROOT,
  )
  assert.equal(selected.ok, true)
  assert.equal(selected.state, 'READY')
  assert.deepEqual(selected.entries.map((item) => item.id), [MICROSOFT_NATIVE_VRAM_N3_COVERAGE_ENTRY.id])
})

test('microsoft_native_vram_n3_module_export_glue_accepts_exact_projection_and_all_targets', () => {
  const entry = MICROSOFT_NATIVE_VRAM_N3_MODULE_EXPORT_GLUE_ENTRY
  const map = { schema_version: 2, entries: [entry] }
  const root = moduleExportGlueRoot(entry)
  const selected = selectCoverageEntries(map, entry.files, root, {
    baseRevision: 'e'.repeat(40),
    readBaseFile: moduleExportGlueBaseReader(),
  })
  assert.equal(selected.ok, true)
  assert.equal(selected.state, 'READY')
  assert.deepEqual(selected.entries.map((item) => item.id), [entry.id])

  const calls = []
  const execution = runCoveragePlan(selected.entries, {
    root,
    spawn(command, args, options) {
      calls.push({ command, args, options })
      return { status: 0 }
    },
  })
  assert.equal(execution.ok, true)
  assert.deepEqual(calls, [{
    command: 'cargo',
    args: ['test', '-p', 'ramshared-tier', '--all-targets'],
    options: { cwd: root, shell: false, stdio: 'inherit' },
  }])
})

test('main_all_accepts_unchanged_exact_module_export_glue_projection', () => {
  const entry = MICROSOFT_NATIVE_VRAM_N3_MODULE_EXPORT_GLUE_ENTRY
  const stableSource = 'pub mod cascade;\npub mod priority;\npub mod n3_state;\npub mod nbd_readiness;\n'
  const selected = selectCoverageEntries(
    { schema_version: 2, entries: [entry] },
    entry.files,
    moduleExportGlueRoot(entry, stableSource),
    {
      baseRevision: 'e'.repeat(40),
      readBaseFile: () => stableSource,
    },
  )
  assert.equal(selected.ok, true)
  assert.equal(selected.state, 'READY')
  assert.deepEqual(selected.entries.map((item) => item.id), [entry.id])

  const incompleteSource = 'pub mod cascade;\npub mod priority;\npub mod n3_state;\n'
  const incomplete = selectCoverageEntries(
    { schema_version: 2, entries: [entry] },
    entry.files,
    moduleExportGlueRoot(entry, incompleteSource),
    {
      baseRevision: 'f'.repeat(40),
      readBaseFile: () => incompleteSource,
    },
  )
  assert.equal(incomplete.ok, false)
  assert.equal(incomplete.errors.some((item) => item.rule === 'module-export-glue-differential-not-proven'), true)
})

test('microsoft_native_vram_n3_module_export_glue_rejects_non_glue_changes_and_wrong_scope', () => {
  const entry = MICROSOFT_NATIVE_VRAM_N3_MODULE_EXPORT_GLUE_ENTRY
  const base = 'pub mod cascade;\npub mod priority;\n'
  const invalidSources = [
    `${base}pub fn hidden() {}\npub mod n3_state;\npub mod nbd_readiness;\n`,
    `${base}pub static LIMIT: usize = 1;\npub mod n3_state;\npub mod nbd_readiness;\n`,
    `${base}pub unsafe fn hidden() {}\npub mod n3_state;\npub mod nbd_readiness;\n`,
    `${base}pub mod n3_state;\npub mod nbd_readiness;\npub mod extra;\n`,
    `pub mod changed;\npub mod priority;\npub mod n3_state;\npub mod nbd_readiness;\n`,
    `${base}pub mod n3_state; \npub mod nbd_readiness;\n`,
    `${base}pub mod n3_state;\n`,
  ]
  for (const headSource of invalidSources) {
    const result = selectCoverageEntries(
      { schema_version: 2, entries: [entry] },
      entry.files,
      moduleExportGlueRoot(entry, headSource),
      { baseRevision: 'f'.repeat(40), readBaseFile: moduleExportGlueBaseReader(base) },
    )
    assert.equal(result.ok, false)
    assert.equal(result.errors.some((item) => item.rule === 'module-export-glue-differential-not-proven'), true)
  }

  const wrongPath = {
    ...entry,
    files: ['crates/ramshared-tier/src/other.rs'],
  }
  const wrongPathResult = validateCoverageMap(
    { schema_version: 2, entries: [wrongPath] },
    moduleExportGlueRoot(wrongPath),
  )
  assert.equal(wrongPathResult.ok, false)
  assert.equal(wrongPathResult.errors.some((item) => item.rule === 'module-export-glue-path-invalid'), true)

  const wrongDeclaration = { ...entry, declaration: 'pub mod other;' }
  const wrongDeclarationResult = validateCoverageMap(
    { schema_version: 2, entries: [wrongDeclaration] },
    moduleExportGlueRoot(wrongDeclaration),
  )
  assert.equal(wrongDeclarationResult.ok, false)
  assert.equal(
    wrongDeclarationResult.errors.some((item) => item.rule === 'module-export-glue-declaration-invalid'),
    true,
  )
})

test('wsl2_nbd_product_readiness_has_exact_coverage_owner_and_named_tests', () => {
  const map = JSON.parse(readFileSync(path.join(REPOSITORY_ROOT, 'docs', 'governance', 'rust-slice-coverage.json'), 'utf8'))
  const entry = map.entries.find((item) => item.id === WSL2_NBD_PRODUCT_READINESS_COVERAGE_ENTRY.id)
  assert.deepEqual(entry, WSL2_NBD_PRODUCT_READINESS_COVERAGE_ENTRY)

  const selected = selectCoverageEntries(
    { schema_version: 2, entries: [entry] },
    WSL2_NBD_PRODUCT_READINESS_COVERAGE_ENTRY.files,
    REPOSITORY_ROOT,
  )
  assert.equal(selected.ok, true)
  assert.equal(selected.state, 'READY')
  assert.deepEqual(selected.entries.map((item) => item.id), [WSL2_NBD_PRODUCT_READINESS_COVERAGE_ENTRY.id])

  const readinessTests = readFileSync(
    path.join(REPOSITORY_ROOT, 'crates', 'ramshared-tier', 'tests', 'nbd_product_readiness.rs'),
    'utf8',
  )
  for (const name of WSL2_NBD_PRODUCT_READINESS_TESTS) {
    assert.match(readinessTests, new RegExp(`\\bfn ${name}\\s*\\(`))
  }

  const cascadeTests = readFileSync(
    path.join(REPOSITORY_ROOT, 'crates', 'ramshared-tier', 'src', 'cascade.rs'),
    'utf8',
  )
  assert.match(cascadeTests, /\bfn ublk_service_is_not_a_product_dependency\s*\(/)
})

test('comment_language_measured_rust_files_keep_exact_ownership_boundaries', () => {
  const map = JSON.parse(readFileSync(path.join(REPOSITORY_ROOT, 'docs', 'governance', 'rust-slice-coverage.json'), 'utf8'))
  const entry = map.entries.find((item) => item.id === COMMENT_LANGUAGE_COVERAGE_ENTRY.id)
  assert.deepEqual(entry, COMMENT_LANGUAGE_COVERAGE_ENTRY)

  const ownerOnlyMap = { schema_version: 2, entries: [entry] }
  const covered = selectCoverageEntries(ownerOnlyMap, COMMENT_LANGUAGE_HIGH_COVERAGE_FILES, REPOSITORY_ROOT)
  assert.equal(covered.ok, true)
  assert.equal(covered.state, 'READY')
  assert.deepEqual(covered.entries.map((item) => item.id), [COMMENT_LANGUAGE_COVERAGE_ENTRY.id])

  const blocked = selectCoverageEntries(ownerOnlyMap, COMMENT_LANGUAGE_BLOCKED_FILES, REPOSITORY_ROOT)
  assert.equal(blocked.ok, false)
  assert.deepEqual(
    blocked.errors.filter((item) => item.rule === 'changed-rust-file-unmapped').map((item) => item.detail),
    [...COMMENT_LANGUAGE_BLOCKED_FILES].sort(),
  )

  const featureOwned = selectCoverageEntries(
    {
      schema_version: 2,
      entries: [
        map.entries.find((item) => item.id === MEMORY_BROKER_AGENT_CLI_COVERAGE_ENTRY.id),
        map.entries.find((item) => item.id === CASCADE_TRANSPORT_ORCHESTRATION_COVERAGE_ENTRY.id),
        map.entries.find((item) => item.id === CASCADE_LIFECYCLE_CLI_COVERAGE_ENTRY.id),
        map.entries.find((item) => item.id === WSL2_CONN_COVERAGE_ENTRY.id),
      ],
    },
    COMMENT_LANGUAGE_FEATURE_OWNED_FILES,
    REPOSITORY_ROOT,
  )
  assert.equal(featureOwned.ok, true)
  assert.equal(featureOwned.state, 'READY')
  assert.deepEqual(
    featureOwned.entries.map((item) => item.id),
    [
      'cascade-lifecycle-observability',
      'cascade-transport-orchestration',
      'memory-broker-agent-cli',
      'wsl2-cascade-connection-transport',
    ],
  )
})

test('wsl2_control_plane_requires_exact_four_file_coverage_owner', () => {
  const map = JSON.parse(readFileSync(path.join(REPOSITORY_ROOT, 'docs', 'governance', 'rust-slice-coverage.json'), 'utf8'))
  const entry = map.entries.find((item) => item.id === WSL2_CONTROL_PLANE_COVERAGE_ENTRY.id)
  assert.deepEqual(entry, WSL2_CONTROL_PLANE_COVERAGE_ENTRY)

  const selected = selectCoverageEntries(
    { schema_version: 2, entries: [entry] },
    WSL2_CONTROL_PLANE_COVERAGE_ENTRY.files,
    REPOSITORY_ROOT,
  )
  assert.equal(selected.ok, true)
  assert.equal(selected.state, 'READY')
  assert.deepEqual(selected.entries.map((item) => item.id), [WSL2_CONTROL_PLANE_COVERAGE_ENTRY.id])
})

test('memory_broker_agent_cli_requires_exact_coverage_owner_and_named_tests', () => {
  const map = JSON.parse(readFileSync(path.join(REPOSITORY_ROOT, 'docs', 'governance', 'rust-slice-coverage.json'), 'utf8'))
  const entry = map.entries.find((item) => item.id === MEMORY_BROKER_AGENT_CLI_COVERAGE_ENTRY.id)
  assert.deepEqual(entry, MEMORY_BROKER_AGENT_CLI_COVERAGE_ENTRY)

  const selected = selectCoverageEntries(
    { schema_version: 2, entries: [entry] },
    MEMORY_BROKER_AGENT_CLI_COVERAGE_ENTRY.files,
    REPOSITORY_ROOT,
  )
  assert.equal(selected.ok, true)
  assert.equal(selected.state, 'READY')
  assert.deepEqual(selected.entries.map((item) => item.id), [MEMORY_BROKER_AGENT_CLI_COVERAGE_ENTRY.id])

  const processTests = readFileSync(
    path.join(REPOSITORY_ROOT, 'crates', 'ramshared-agent', 'tests', 'agent_cli.rs'),
    'utf8',
  )
  for (const name of MEMORY_BROKER_AGENT_CLI_PROCESS_TESTS) {
    assert.match(processTests, new RegExp(`\\bfn ${name}\\s*\\(`))
  }
  const mainTests = readFileSync(
    path.join(REPOSITORY_ROOT, 'crates', 'ramshared-agent', 'src', 'main.rs'),
    'utf8',
  )
  for (const name of MEMORY_BROKER_AGENT_CLI_MAIN_TESTS) {
    assert.match(mainTests, new RegExp(`\\bfn ${name}\\s*\\(`))
  }
})

test('memory_broker_wsl2d_daemon_requires_exact_coverage_owner_and_named_tests', () => {
  const map = JSON.parse(readFileSync(path.join(REPOSITORY_ROOT, 'docs', 'governance', 'rust-slice-coverage.json'), 'utf8'))
  const entry = map.entries.find((item) => item.id === MEMORY_BROKER_WSL2D_DAEMON_COVERAGE_ENTRY.id)
  assert.deepEqual(entry, MEMORY_BROKER_WSL2D_DAEMON_COVERAGE_ENTRY)

  const selected = selectCoverageEntries(
    { schema_version: 2, entries: [entry] },
    MEMORY_BROKER_WSL2D_DAEMON_COVERAGE_ENTRY.files,
    REPOSITORY_ROOT,
  )
  assert.equal(selected.ok, true)
  assert.equal(selected.state, 'READY')
  assert.deepEqual(
    selected.entries.map((item) => item.id),
    [MEMORY_BROKER_WSL2D_DAEMON_COVERAGE_ENTRY.id],
  )

  const mainTests = readFileSync(
    path.join(REPOSITORY_ROOT, 'crates', 'ramshared-wsl2d', 'src', 'main.rs'),
    'utf8',
  )
  for (const name of MEMORY_BROKER_WSL2D_DAEMON_MAIN_TESTS) {
    assert.match(mainTests, new RegExp(`\\bfn ${name}\\s*\\(`))
  }
  const processTests = readFileSync(
    path.join(REPOSITORY_ROOT, 'crates', 'ramshared-wsl2d', 'tests', 'daemon_cli.rs'),
    'utf8',
  )
  for (const name of MEMORY_BROKER_WSL2D_DAEMON_PROCESS_TESTS) {
    assert.match(processTests, new RegExp(`\\bfn ${name}\\s*\\(`))
  }
})

test('comment_language_test_only_localization_requires_immutable_base_proof', () => {
  const map = JSON.parse(readFileSync(path.join(REPOSITORY_ROOT, 'docs', 'governance', 'rust-slice-coverage.json'), 'utf8'))
  const entry = map.entries.find((item) => item.id === COMMENT_LANGUAGE_TEST_ONLY_ENTRY.id)
  assert.deepEqual(entry, COMMENT_LANGUAGE_TEST_ONLY_ENTRY)

  const revision = execFileSync('git', ['rev-parse', 'HEAD'], {
    cwd: REPOSITORY_ROOT,
    encoding: 'utf8',
  }).trim()
  const selected = selectCoverageEntries(
    { schema_version: 2, entries: [entry] },
    entry.files,
    REPOSITORY_ROOT,
    { baseRevision: revision },
  )
  assert.equal(selected.ok, true)
  assert.equal(selected.state, 'READY')
  assert.deepEqual(selected.entries.map((item) => item.id), [entry.id])
})

test('memory_broker_backend_has_exact_coverage_and_pinned_gpu_relocation_owners', () => {
  const map = JSON.parse(readFileSync(
    path.join(REPOSITORY_ROOT, 'docs', 'governance', 'rust-slice-coverage.json'),
    'utf8',
  ))
  const coverage = map.entries.find((item) => item.id === MEMORY_BROKER_WSL2D_BACKEND_COVERAGE_ENTRY.id)
  const relocation = map.entries.find(
    (item) => item.id === MEMORY_BROKER_WSL2D_BACKEND_GPU_RELOCATION_ENTRY.id,
  )
  assert.deepEqual(coverage, MEMORY_BROKER_WSL2D_BACKEND_COVERAGE_ENTRY)
  assert.deepEqual(relocation, MEMORY_BROKER_WSL2D_BACKEND_GPU_RELOCATION_ENTRY)

  const selected = selectCoverageEntries(
    { schema_version: 2, entries: [coverage, relocation] },
    [coverage.files[0], relocation.verification.ignored_test_source],
    REPOSITORY_ROOT,
  )
  assert.equal(selected.ok, true)
  assert.equal(selected.state, 'READY')
  assert.deepEqual(selected.entries.map((item) => item.id), [coverage.id, relocation.id])
})

test('wsl2_connection_transport_requires_exact_canonical_coverage', () => {
  const map = JSON.parse(readFileSync(path.join(REPOSITORY_ROOT, 'docs', 'governance', 'rust-slice-coverage.json'), 'utf8'))
  const entry = map.entries.find((item) => item.id === WSL2_CONN_COVERAGE_ENTRY.id)
  assert.deepEqual(entry, WSL2_CONN_COVERAGE_ENTRY)

  const selected = selectCoverageEntries(
    { schema_version: 2, entries: [entry] },
    WSL2_CONN_COVERAGE_ENTRY.files,
    REPOSITORY_ROOT,
  )
  assert.equal(selected.ok, true)
  assert.equal(selected.state, 'READY')
  assert.deepEqual(selected.entries.map((item) => item.id), [WSL2_CONN_COVERAGE_ENTRY.id])
})

test('cascade_lifecycle_cli_dispatch_requires_exact_coverage_owner_and_named_tests', () => {
  const map = JSON.parse(readFileSync(path.join(REPOSITORY_ROOT, 'docs', 'governance', 'rust-slice-coverage.json'), 'utf8'))
  const entry = map.entries.find((item) => item.id === CASCADE_LIFECYCLE_CLI_COVERAGE_ENTRY.id)
  assert.deepEqual(entry, CASCADE_LIFECYCLE_CLI_COVERAGE_ENTRY)

  const selected = selectCoverageEntries(
    { schema_version: 2, entries: [entry] },
    ['crates/ramshared-cli/src/main.rs'],
    REPOSITORY_ROOT,
  )
  assert.equal(selected.ok, true)
  assert.equal(selected.state, 'READY')
  assert.deepEqual(selected.entries.map((item) => item.id), [CASCADE_LIFECYCLE_CLI_COVERAGE_ENTRY.id])

  const mainTests = readFileSync(
    path.join(REPOSITORY_ROOT, 'crates', 'ramshared-cli', 'src', 'main.rs'),
    'utf8',
  )
  for (const name of CASCADE_LIFECYCLE_CLI_MAIN_TESTS) {
    assert.match(mainTests, new RegExp(`\\bfn ${name}\\s*\\(`))
  }
  const processTests = readFileSync(
    path.join(REPOSITORY_ROOT, 'crates', 'ramshared-cli', 'tests', 'cli_dispatch.rs'),
    'utf8',
  )
  for (const name of CASCADE_LIFECYCLE_CLI_PROCESS_TESTS) {
    assert.match(processTests, new RegExp(`\\bfn ${name}\\s*\\(`))
  }
})

test('cascade_transport_orchestration_requires_exact_coverage_owner_and_named_tests', () => {
  const map = JSON.parse(readFileSync(path.join(REPOSITORY_ROOT, 'docs', 'governance', 'rust-slice-coverage.json'), 'utf8'))
  const entry = map.entries.find((item) => item.id === CASCADE_TRANSPORT_ORCHESTRATION_COVERAGE_ENTRY.id)
  assert.deepEqual(entry, CASCADE_TRANSPORT_ORCHESTRATION_COVERAGE_ENTRY)

  const selected = selectCoverageEntries(
    { schema_version: 2, entries: [entry] },
    CASCADE_TRANSPORT_ORCHESTRATION_COVERAGE_ENTRY.files,
    REPOSITORY_ROOT,
  )
  assert.equal(selected.ok, true)
  assert.equal(selected.state, 'READY')
  assert.deepEqual(selected.entries.map((item) => item.id), [CASCADE_TRANSPORT_ORCHESTRATION_COVERAGE_ENTRY.id])

  const cascadeSource = readFileSync(
    path.join(REPOSITORY_ROOT, 'crates', 'ramshared-cli', 'src', 'cascade', 'cascade_io.rs'),
    'utf8',
  )
  for (const name of CASCADE_TRANSPORT_ORCHESTRATION_CASCADE_TESTS) {
    assert.match(cascadeSource, new RegExp(`\\bfn ${name}\\s*\\(`))
  }
  const runnerSource = readFileSync(
    path.join(REPOSITORY_ROOT, 'crates', 'ramshared-cli', 'src', 'bounded_process.rs'),
    'utf8',
  )
  for (const name of CASCADE_TRANSPORT_ORCHESTRATION_RUNNER_TESTS) {
    assert.match(runnerSource, new RegExp(`\\bfn ${name}\\s*\\(`))
  }
})

test('windows_platform_entry_requires_exact_feature_spec_and_named_checks', () => {
  const entry = platformEntry()
  const map = { schema_version: 2, entries: [entry] }
  const root = platformRoot(entry)

  const validation = validateCoverageMap(map, root)
  assert.equal(validation.ok, true)
  const selection = selectCoverageEntries(map, entry.files, root)
  assert.equal(selection.ok, true)
  assert.equal(selection.state, 'READY')
  assert.deepEqual(selection.entries.map((item) => item.id), [entry.id])

  mkdirSync(path.join(root, 'docs', 'governance'), { recursive: true })
  writeFileSync(path.join(root, 'docs', 'governance', 'rust-slice-coverage.json'), `${JSON.stringify(map)}\n`)
  const output = []
  assert.equal(main(['--all'], {
    root,
    print: (line) => output.push(line),
    error: () => {},
  }), 0)
  assert.equal(output.includes(`RUST_SLICE_PLATFORM_E2E_REQUIRED=${entry.id}`), true)

  const mismatchedSpec = platformRoot(entry)
  writeFileSync(
    path.join(mismatchedSpec, 'docs', 'specs', 'no-milestone', 'fixture', 'SPEC.md'),
    embeddedOwnership('rust-slice-platform-e2e-v1', {
      ...platformDeclaration(entry),
      files: ['crates/fixture/src/other.rs'],
    }),
  )
  const mismatch = validateCoverageMap(map, mismatchedSpec)
  assert.equal(mismatch.ok, false)
  assert.equal(mismatch.errors.some((item) => item.rule === 'platform-spec-contract-mismatch'), true)
})

test('windows_platform_entry_refuses_static_harness_outside_windows_ci', () => {
  const entry = platformEntry()
  const map = { schema_version: 2, entries: [entry] }
  const result = validateCoverageMap(map, platformRoot(entry, { wrapperIncludesStatic: false }))
  assert.equal(result.ok, false)
  assert.equal(result.errors.some((item) => item.rule === 'platform-static-harness-not-run'), true)
})

test('structural_contract_accepts_only_module_surface_and_runs_package_tests', () => {
  const entry = structuralEntry()
  const map = { schema_version: 2, entries: [entry] }
  const root = structuralRoot(entry)
  const validation = validateCoverageMap(map, root)
  assert.equal(validation.ok, true)

  const selection = selectCoverageEntries(map, entry.files, root)
  assert.equal(selection.ok, true)
  assert.equal(selection.state, 'READY')

  const calls = []
  const execution = runCoveragePlan(selection.entries, {
    root,
    spawn(command, args, options) {
      calls.push({ command, args, options })
      return { status: 0 }
    },
  })
  assert.equal(execution.ok, true)
  assert.deepEqual(calls, [{
    command: 'cargo',
    args: ['test', '-p', 'fixture', '--lib'],
    options: { cwd: root, shell: false, stdio: 'inherit' },
  }])

  const duplicated = {
    ...entry,
    verifications: [entry.verifications[0], entry.verifications[0]],
  }
  const failedCalls = []
  const failed = runCoveragePlan([duplicated], {
    root,
    spawn(command, args, options) {
      failedCalls.push({ command, args, options })
      return { status: 1 }
    },
  })
  assert.equal(failed.ok, false)
  assert.equal(failedCalls.length, 1)
  assert.equal(failed.errors.some((item) => item.rule === 'structural-package-test-failed'), true)
})

test('structural_contract_refuses_executable_or_malformed_rust', () => {
  const entry = structuralEntry()
  for (const source of [
    'pub fn hidden_business_logic() {}\n',
    'pub const LIMIT: usize = 1;\n',
    'pub use policy::{Decision;\n',
    '# not_an_attribute\npub mod policy;\n',
    'pub use "literal";\n',
    'pub use 1;\n',
    'pub use policy();\n',
    'pub use ;\n',
    'pub enum Decision {}\n',
    '// comments only\n',
  ]) {
    const result = validateCoverageMap(
      { schema_version: 2, entries: [entry] },
      structuralRoot(entry, source),
    )
    assert.equal(result.ok, false)
    assert.equal(result.errors.some((item) => item.rule === 'structural-rust-source-invalid'), true)
  }

  const invalidVerification = structuralEntry()
  invalidVerification.verifications[0].package = 'other'
  const invalidVerificationResult = validateCoverageMap(
    { schema_version: 2, entries: [invalidVerification] },
    structuralRoot(invalidVerification),
  )
  assert.equal(invalidVerificationResult.ok, false)
  assert.equal(invalidVerificationResult.errors.some((item) => item.rule === 'structural-verification-invalid'), true)
  assert.equal(invalidVerificationResult.errors.some((item) => item.rule === 'structural-source-files-mismatch'), true)

  const missingContractRoot = fixtureRoot('no structural declaration\n')
  writeFixtureFile(missingContractRoot, entry.files[0], 'pub mod policy;\n')
  const missingContract = validateCoverageMap({ schema_version: 2, entries: [entry] }, missingContractRoot)
  assert.equal(missingContract.errors.some((item) => item.rule === 'structural-spec-contract-missing'), true)

  const mismatchedRoot = structuralRoot(entry)
  writeFixtureFile(
    mismatchedRoot,
    'docs/specs/no-milestone/fixture/SPEC.md',
    embeddedOwnership('rust-slice-structural-contract-v1', {
      ...structuralDeclaration(entry),
      files: ['crates/fixture/src/other.rs'],
    }),
  )
  const mismatched = validateCoverageMap({ schema_version: 2, entries: [entry] }, mismatchedRoot)
  assert.equal(mismatched.errors.some((item) => item.rule === 'structural-spec-contract-mismatch'), true)
})

test('windows_autonomous_sources_have_exact_structural_or_platform_owners', () => {
  const map = JSON.parse(readFileSync(path.join(REPOSITORY_ROOT, 'docs', 'governance', 'rust-slice-coverage.json'), 'utf8'))
  const ownerIds = new Set([
    'windows-autonomous-platform-e2e-rust',
    'windows-autonomous-structural-rust',
  ])
  const scopedMap = { ...map, entries: map.entries.filter((entry) => ownerIds.has(entry.id)) }
  const files = [
    'crates/ramshared-broker/src/lib.rs',
    'crates/ramshared-winbroker/src/main.rs',
    'crates/ramshared-winbroker/src/service.rs',
    'crates/ramshared-winsvc/src/bin/ramshared-service-sid-probe.rs',
    'crates/ramshared-winsvc/src/cuda_probe.rs',
    'crates/ramshared-winsvc/src/lib.rs',
  ]
  const selected = selectCoverageEntries(scopedMap, files, REPOSITORY_ROOT)
  assert.equal(selected.ok, true)
  assert.deepEqual(selected.entries.map((entry) => entry.id), [
    'windows-autonomous-platform-e2e-rust',
    'windows-autonomous-structural-rust',
  ])
})

test('localization_differential_accepts_comment_only_source_change', () => {
  const entry = localizationEntry()
  const map = { schema_version: 2, entries: [entry] }
  const root = localizationRoot(entry, '// English comment\npub fn policy() {}\n')
  const result = selectCoverageEntries(map, entry.files, root, {
    baseRevision: 'a'.repeat(40),
    readBaseFile() {
      return '// Coment\u00e1rio anterior\npub fn policy() {}\n'
    },
  })
  assert.equal(result.ok, true)
  assert.equal(result.state, 'READY')
  assert.deepEqual(result.entries.map((item) => item.id), [entry.id])
})

test('localization_differential_refuses_semantic_change_or_missing_base', () => {
  const entry = localizationEntry()
  const map = { schema_version: 2, entries: [entry] }
  const root = localizationRoot(entry, 'pub fn policy() { eprintln!("English"); }\n')

  const semantic = selectCoverageEntries(map, entry.files, root, {
    baseRevision: 'b'.repeat(40),
    readBaseFile() {
      return 'pub fn policy() { eprintln!("Portuguese"); }\n'
    },
  })
  assert.equal(semantic.ok, false)
  assert.equal(semantic.errors.some((item) => item.rule === 'localization-differential-not-proven'), true)

  const missingBase = selectCoverageEntries(map, entry.files, root)
  assert.equal(missingBase.ok, false)
  assert.equal(missingBase.errors.some((item) => item.rule === 'localization-differential-base-required'), true)

  const invalidBase = selectCoverageEntries(map, entry.files, root, { baseRevision: 'not-a-sha' })
  assert.equal(invalidBase.ok, false)
  assert.equal(invalidBase.errors.some((item) => item.rule === 'localization-differential-base-invalid'), true)
})

test('test_only_localization_differential_accepts_declared_cfg_test_change', () => {
  const entry = testOnlyLocalizationEntry()
  const map = { schema_version: 2, entries: [entry] }
  const root = testOnlyLocalizationRoot(entry)
  const selected = selectCoverageEntries(map, entry.files, root, {
    baseRevision: 'd'.repeat(40),
    readBaseFile: testOnlyBaseReader(entry),
  })
  assert.equal(selected.ok, true)
  assert.equal(selected.state, 'READY')
  assert.deepEqual(selected.entries.map((item) => item.id), [entry.id])

  const calls = []
  const execution = runCoveragePlan(selected.entries, {
    root,
    spawn(command, args, options) {
      calls.push({ command, args, options })
      return { status: 0 }
    },
  })
  assert.equal(execution.ok, true)
  assert.deepEqual(calls, [{
    command: 'cargo',
    args: ['test', '-p', 'fixture', '--lib'],
    options: { cwd: root, shell: false, stdio: 'inherit' },
  }])

  mkdirSync(path.join(root, 'docs', 'governance'), { recursive: true })
  writeFileSync(path.join(root, 'docs', 'governance', 'rust-slice-coverage.json'), `${JSON.stringify(map)}\n`)
  const staticOutput = []
  assert.equal(main(['--all'], {
    root,
    print: (line) => staticOutput.push(line),
    error: () => {},
  }), 0)
  assert.equal(staticOutput.includes('RUST_SLICE_TEST_ONLY_LOCALIZATION_REQUIRED=fixture-test-only-localization'), true)
  assert.equal(staticOutput.includes('RUST_SLICE_TEST_ONLY_LOCALIZATION_BASE_PROOF_DEFERRED=fixture-test-only-localization'), true)

  const noBaseErrors = []
  assert.equal(main(['--all', '--run'], {
    root,
    print: () => {},
    error: (line) => noBaseErrors.push(line),
    spawn: () => {
      throw new Error('test-only package test must not run without immutable base proof')
    },
  }), 1)
  assert.equal(noBaseErrors.includes('RUST_SLICE_COVERAGE_ERROR=test-only-differential-base-required'), true)
})

test('ignored_test_relocation_accepts_exact_pinned_provenance', () => {
  const entry = relocatedTestOnlyLocalizationEntry()
  const map = { schema_version: 2, entries: [entry] }
  const root = relocatedTestOnlyLocalizationRoot(entry)

  assert.equal(validateCoverageMap(map, root).ok, true)
  const selected = selectCoverageEntries(map, entry.files, root, {
    baseRevision: 'd'.repeat(40),
    readBaseFile: relocatedTestOnlyBaseReader(entry),
  })
  assert.equal(selected.ok, true)
  assert.equal(selected.state, 'READY')
})

test('ignored_test_relocation_global_owner_relation_allows_one_line_owner_and_one_proof', () => {
  const relocation = relocationVariant({
    id: 'fixture-relocation-owner',
    spec: 'docs/specs/no-milestone/relocation/SPEC.md',
  })
  const coverage = lineOwnerFor(
    relocation.files[0],
    'fixture-line-owner',
    'docs/specs/no-milestone/coverage/SPEC.md',
  )
  const root = relocationOwnershipRoot([relocation])
  writeFixtureFile(root, coverage.spec, `\`\`\`bash\n${coverage.command.join(' ')}\n\`\`\`\n`)
  assert.equal(validateCoverageMap({ schema_version: 2, entries: [coverage, relocation] }, root).ok, true)

  const secondLine = lineOwnerFor(
    relocation.files[0],
    'fixture-second-line-owner',
    'docs/specs/no-milestone/coverage-second/SPEC.md',
  )
  writeFixtureFile(root, secondLine.spec, `\`\`\`bash\n${secondLine.command.join(' ')}\n\`\`\`\n`)
  const duplicateLine = validateCoverageMap(
    { schema_version: 2, entries: [coverage, secondLine, relocation] },
    root,
  )
  assert.equal(duplicateLine.ok, false)
  assert.equal(duplicateLine.errors.some((item) => item.rule === 'ignored-test-relocation-production-owner-conflict'), true)
})

test('global_line_coverage_ownership_refuses_duplicate_pure_owners_including_cli_all', () => {
  const source = 'crates/fixture/src/policy.rs'
  const first = lineOwnerFor(source, 'fixture-line-first', 'docs/specs/no-milestone/line-first/SPEC.md')
  const second = lineOwnerFor(source, 'fixture-line-second', 'docs/specs/no-milestone/line-second/SPEC.md')
  const root = fixtureRoot(`\`\`\`bash\n${first.command.join(' ')}\n\`\`\`\n`)
  writeFixtureFile(root, first.spec, `\`\`\`bash\n${first.command.join(' ')}\n\`\`\`\n`)
  writeFixtureFile(root, second.spec, `\`\`\`bash\n${second.command.join(' ')}\n\`\`\`\n`)
  const map = { schema_version: 2, entries: [first, second] }
  const validation = validateCoverageMap(map, root)
  assert.equal(validation.ok, false)
  assert.equal(validation.errors.some((item) => item.rule === 'line-coverage-production-owner-duplicate'), true)

  writeFixtureFile(root, 'docs/governance/rust-slice-coverage.json', `${JSON.stringify(map)}\n`)
  const output = []
  const errors = []
  assert.equal(main(['--all'], {
    root,
    print: (line) => output.push(line),
    error: (line) => errors.push(line),
  }), 1)
  assert.equal(output.includes('RUST_SLICE_COVERAGE_STATUS=BLOCKED'), true)
  assert.equal(output.includes('RUST_SLICE_COVERAGE_STATUS=READY'), false)
  assert.equal(errors.includes('RUST_SLICE_COVERAGE_ERROR=line-coverage-production-owner-duplicate'), true)
})

test('planner_trust_inputs_require_confined_non_symlink_regular_files', () => {
  const root = mkdtempSync(path.join(tmpdir(), 'ramshared-rust-trust-root-'))
  const outside = mkdtempSync(path.join(tmpdir(), 'ramshared-rust-trust-outside-'))
  const source = 'crates/fixture/src/policy.rs'
  const spec = 'docs/specs/no-milestone/linked/SPEC.md'
  const owner = lineOwnerFor(source, 'fixture-linked-owner', spec)
  writeFixtureFile(outside, 'policy.rs', 'pub fn external_policy() {}\n')
  writeFixtureFile(outside, 'SPEC.md', `\`\`\`bash\n${owner.command.join(' ')}\n\`\`\`\n`)
  mkdirSync(path.dirname(path.join(root, source)), { recursive: true })
  mkdirSync(path.dirname(path.join(root, spec)), { recursive: true })
  symlinkSync(path.join(outside, 'policy.rs'), path.join(root, source))
  symlinkSync(path.join(outside, 'SPEC.md'), path.join(root, spec))
  const validation = validateCoverageMap({ schema_version: 2, entries: [owner] }, root)
  assert.equal(validation.ok, false)
  assert.equal(validation.errors.some((item) => item.rule === 'coverage-file-untrusted'), true)
  assert.equal(validation.errors.some((item) => item.rule === 'coverage-spec-untrusted'), true)

  const cliRoot = plannerRoot()
  const outsideMap = path.join(outside, 'map.json')
  writeFileSync(outsideMap, JSON.stringify(coverageMap()))
  rmSync(path.join(cliRoot, 'docs/governance/rust-slice-coverage.json'))
  symlinkSync(outsideMap, path.join(cliRoot, 'docs/governance/rust-slice-coverage.json'))
  const mapErrors = []
  assert.equal(main(['--all'], {
    root: cliRoot,
    print: () => {},
    error: (line) => mapErrors.push(line),
  }), 1)
  assert.deepEqual(mapErrors, ['RUST_SLICE_COVERAGE_ERROR=coverage-map-read-failed'])

  rmSync(path.join(cliRoot, 'docs/governance/rust-slice-coverage.json'))
  writeFixtureFile(cliRoot, 'docs/governance/rust-slice-coverage.json', `${JSON.stringify(coverageMap())}\n`)
  rmSync(path.join(cliRoot, 'changed.txt'))
  const outsideChanges = path.join(outside, 'changed.txt')
  writeFileSync(outsideChanges, `${source}\n`)
  symlinkSync(outsideChanges, path.join(cliRoot, 'changed.txt'))
  const changedErrors = []
  assert.equal(main(['--changed-files', 'changed.txt'], {
    root: cliRoot,
    print: () => {},
    error: (line) => changedErrors.push(line),
  }), 1)
  assert.deepEqual(changedErrors, ['RUST_SLICE_COVERAGE_ERROR=changed-paths-read-failed'])
})

test('ignored_test_relocation_global_validation_refuses_duplicate_production_and_integration_owners', () => {
  const first = relocationVariant({
    id: 'fixture-relocation-first',
    spec: 'docs/specs/no-milestone/relocation-first/SPEC.md',
  })
  const duplicateProduction = relocationVariant({
    id: 'fixture-relocation-second',
    spec: 'docs/specs/no-milestone/relocation-second/SPEC.md',
    target: 'backend_gpu_second',
  })
  const productionRoot = relocationOwnershipRoot([first, duplicateProduction])
  const productionResult = validateCoverageMap(
    { schema_version: 2, entries: [first, duplicateProduction] },
    productionRoot,
  )
  assert.equal(productionResult.ok, false)
  assert.equal(productionResult.errors.some((item) =>
    item.rule === 'ignored-test-relocation-production-owner-duplicate'), true)

  const duplicateIntegration = relocationVariant({
    id: 'fixture-relocation-third',
    spec: 'docs/specs/no-milestone/relocation-third/SPEC.md',
    source: 'crates/fixture/src/other.rs',
  })
  const integrationRoot = relocationOwnershipRoot([first, duplicateIntegration])
  const integrationResult = validateCoverageMap(
    { schema_version: 2, entries: [first, duplicateIntegration] },
    integrationRoot,
  )
  assert.equal(integrationResult.ok, false)
  assert.equal(integrationResult.errors.some((item) =>
    item.rule === 'ignored-test-relocation-integration-owner-duplicate'), true)
})

test('ignored_test_relocation_global_validation_refuses_wrong_cross_owner_overlap', () => {
  const relocation = relocationVariant({
    id: 'fixture-relocation-cross-owner',
    spec: 'docs/specs/no-milestone/relocation/SPEC.md',
  })
  const localization = {
    id: 'fixture-localization-cross-owner',
    kind: 'rust-localization-comment-differential',
    spec: 'docs/specs/no-milestone/localization/SPEC.md',
    files: relocation.files,
  }
  const root = relocationOwnershipRoot([relocation])
  writeFixtureFile(root, localization.spec, embeddedOwnership(
    'rust-slice-localization-comment-differential-v1',
    localizationDeclaration(localization),
  ))
  const result = validateCoverageMap({ schema_version: 2, entries: [relocation, localization] }, root)
  assert.equal(result.ok, false)
  assert.equal(result.errors.some((item) => item.rule === 'ignored-test-relocation-production-owner-conflict'), true)
})

test('ignored_test_relocation_duplicate_owner_cli_all_is_blocked_and_never_ready', () => {
  const first = relocationVariant({
    id: 'fixture-relocation-cli-first',
    spec: 'docs/specs/no-milestone/relocation-cli-first/SPEC.md',
  })
  const second = relocationVariant({
    id: 'fixture-relocation-cli-second',
    spec: 'docs/specs/no-milestone/relocation-cli-second/SPEC.md',
    target: 'backend_gpu_second',
  })
  const root = relocationOwnershipRoot([first, second])
  writeFixtureFile(root, 'docs/governance/rust-slice-coverage.json', `${JSON.stringify({
    schema_version: 2,
    entries: [first, second],
  })}\n`)
  const output = []
  const errors = []
  assert.equal(main(['--all'], {
    root,
    print: (line) => output.push(line),
    error: (line) => errors.push(line),
  }), 1)
  assert.equal(output.includes('RUST_SLICE_COVERAGE_STATUS=BLOCKED'), true)
  assert.equal(output.includes('RUST_SLICE_COVERAGE_STATUS=READY'), false)
  assert.equal(errors.includes(
    'RUST_SLICE_COVERAGE_ERROR=ignored-test-relocation-production-owner-duplicate',
  ), true)
})

test('ignored_test_relocation_refuses_wrong_package_path_target_or_command', () => {
  const cases = [
    {
      name: 'wrong package',
      mutate(verification) {
        verification.package = 'other'
      },
    },
    {
      name: 'wrong integration path',
      mutate(verification) {
        verification.ignored_test_source = 'crates/fixture/tests/other.rs'
      },
    },
    {
      name: 'wrong integration target',
      mutate(verification) {
        verification.ignored_test_target = 'other'
      },
    },
    {
      name: 'current command target mismatch',
      mutate(verification) {
        verification.ignored_gpu_tests[0].command[6] = 'other'
      },
    },
    {
      name: 'current command name mismatch',
      mutate(verification) {
        verification.ignored_gpu_tests[0].command[7] = 'other_test'
      },
    },
    {
      name: 'historical command mismatch',
      mutate(verification) {
        verification.ignored_gpu_tests[0].historical_command[4] = 'tests::gpu_roundtrip'
      },
    },
  ]

  for (const item of cases) {
    const entry = relocatedTestOnlyLocalizationEntry()
    item.mutate(entry.verification)
    const root = relocatedTestOnlyLocalizationRoot(entry)
    const result = validateCoverageMap({ schema_version: 2, entries: [entry] }, root)
    assert.equal(result.ok, false, item.name)
    assert.equal(result.errors.some((error) => error.rule === 'ignored-test-relocation-verification-invalid'), true, item.name)
  }
})

test('ignored_test_relocation_refuses_missing_or_symlinked_head_test', () => {
  const missingEntry = relocatedTestOnlyLocalizationEntry()
  const missingRoot = relocatedTestOnlyLocalizationRoot(
    missingEntry,
    RELOCATED_TEST_ONLY_INTEGRATION_SOURCE.replace('fn gpu_roundtrip()', 'fn different_test()'),
  )
  const missing = validateCoverageMap({ schema_version: 2, entries: [missingEntry] }, missingRoot)
  assert.equal(missing.ok, false)
  assert.equal(missing.errors.some((item) => item.rule === 'ignored-test-relocation-head-test-missing'), true)

  const symlinkEntry = relocatedTestOnlyLocalizationEntry()
  const symlinkRoot = relocatedTestOnlyLocalizationRoot(symlinkEntry)
  const link = path.join(symlinkRoot, symlinkEntry.verification.ignored_test_source)
  const target = path.join(symlinkRoot, 'outside-backend-gpu.rs')
  writeFileSync(target, RELOCATED_TEST_ONLY_INTEGRATION_SOURCE)
  rmSync(link)
  symlinkSync(target, link)
  const symlinked = validateCoverageMap({ schema_version: 2, entries: [symlinkEntry] }, symlinkRoot)
  assert.equal(symlinked.ok, false)
  assert.equal(symlinked.errors.some((item) => item.rule === 'ignored-test-relocation-head-source-invalid'), true)
})

test('ignored_test_relocation_refuses_undeclared_import_drift', () => {
  const entry = relocatedTestOnlyLocalizationEntry()
  const root = relocatedTestOnlyLocalizationRoot(
    entry,
    RELOCATED_TEST_ONLY_INTEGRATION_SOURCE.replace('use fixture::Gpu;', 'use foreign::Gpu;'),
  )
  const result = validateCoverageMap({ schema_version: 2, entries: [entry] }, root)
  assert.equal(result.ok, false)
  assert.equal(result.errors.some((item) => item.rule === 'ignored-test-relocation-imports-mismatch'), true)

  const cfgEntry = relocatedTestOnlyLocalizationEntry()
  const cfgRoot = relocatedTestOnlyLocalizationRoot(
    cfgEntry,
    `#![cfg(any())]\n${RELOCATED_TEST_ONLY_INTEGRATION_SOURCE}`,
  )
  const cfgResult = validateCoverageMap({ schema_version: 2, entries: [cfgEntry] }, cfgRoot)
  assert.equal(cfgResult.ok, false)
  assert.equal(cfgResult.errors.some((item) => item.rule === 'ignored-test-relocation-head-source-invalid'), true)
})

test('ignored_test_relocation_refuses_base_sha_missing_test_semantic_drift_or_evidence_mismatch', () => {
  const cases = [
    {
      name: 'base source sha mismatch',
      base: RELOCATED_TEST_ONLY_BASE_SOURCE,
      head: RELOCATED_TEST_ONLY_INTEGRATION_SOURCE,
      evidence: null,
      pinBaseSha: false,
      rule: 'ignored-test-relocation-base-sha-mismatch',
    },
    {
      name: 'missing base test',
      base: RELOCATED_TEST_ONLY_BASE_SOURCE.replace('fn gpu_roundtrip()', 'fn different_test()'),
      head: RELOCATED_TEST_ONLY_INTEGRATION_SOURCE,
      evidence: null,
      rule: 'ignored-test-relocation-base-test-missing',
    },
    {
      name: 'renamed local',
      base: RELOCATED_TEST_ONLY_BASE_SOURCE,
      head: RELOCATED_TEST_ONLY_INTEGRATION_SOURCE.replaceAll('payload', 'renamed'),
      evidence: null,
      rule: 'ignored-test-relocation-not-proven',
    },
    {
      name: 'removed logging',
      base: RELOCATED_TEST_ONLY_BASE_SOURCE,
      head: RELOCATED_TEST_ONLY_INTEGRATION_SOURCE.replace('    eprintln!("stable log: {payload}");\n', ''),
      evidence: null,
      rule: 'ignored-test-relocation-not-proven',
    },
    {
      name: 'removed assertion',
      base: RELOCATED_TEST_ONLY_BASE_SOURCE,
      head: RELOCATED_TEST_ONLY_INTEGRATION_SOURCE.replace(
        '    assert_eq!(payload, 7u64, "stable message: {payload}");\n',
        '',
      ),
      evidence: null,
      rule: 'ignored-test-relocation-not-proven',
    },
    {
      name: 'changed message',
      base: RELOCATED_TEST_ONLY_BASE_SOURCE,
      head: RELOCATED_TEST_ONLY_INTEGRATION_SOURCE.replace('stable message', 'changed message'),
      evidence: null,
      rule: 'ignored-test-relocation-not-proven',
    },
    {
      name: 'historical evidence mismatch',
      base: RELOCATED_TEST_ONLY_BASE_SOURCE,
      head: RELOCATED_TEST_ONLY_INTEGRATION_SOURCE,
      evidence: '- `cargo test -p fixture --test backend_gpu gpu_roundtrip -- --ignored --test-threads=1`: **PASS**.',
      rule: 'ignored-test-relocation-evidence-missing',
    },
  ]

  for (const item of cases) {
    const entry = relocatedTestOnlyLocalizationEntry()
    if (item.pinBaseSha !== false) {
      entry.base_source_sha256 = createHash('sha256').update(item.base).digest('hex')
    } else {
      entry.base_source_sha256 = '0'.repeat(64)
    }
    const root = relocatedTestOnlyLocalizationRoot(entry, item.head)
    const evidence = item.evidence ?? entry.verification.ignored_gpu_tests
      .map((ignored) => `- \`${ignored.historical_command.join(' ')}\`: **PASS**.`)
      .join('\n')
    const result = selectCoverageEntries({ schema_version: 2, entries: [entry] }, entry.files, root, {
      baseRevision: 'd'.repeat(40),
      readBaseFile: relocatedTestOnlyBaseReader(entry, item.base, evidence),
    })
    assert.equal(result.ok, false, item.name)
    assert.equal(result.errors.some((error) => error.rule === item.rule), true, item.name)
  }
})

test('test_only_localization_differential_refuses_spoofed_or_production_change', () => {
  const cases = [
    {
      name: 'cfg test string spoof',
      head: TEST_ONLY_HEAD_SOURCE.replace(
        '#[cfg(test)]\nmod tests {',
        'const SPOOF: &str = "#[cfg(test)] mod tests {";\nmod tests {',
      ),
      rule: 'test-only-declared-module-missing',
    },
    {
      name: 'production string',
      head: TEST_ONLY_HEAD_SOURCE.replace('stable { #[cfg(test)] }', 'changed { #[cfg(test)] }'),
      rule: 'test-only-production-projection-not-proven',
    },
    {
      name: 'production control token',
      head: TEST_ONLY_HEAD_SOURCE.replace("b'{'", "b'}'"),
      rule: 'test-only-production-projection-not-proven',
    },
    {
      name: 'production c raw string',
      head: TEST_ONLY_HEAD_SOURCE.replace('literal text', 'changed literal text'),
      rule: 'test-only-production-projection-not-proven',
    },
    {
      name: 'moved cfg attribute',
      head: TEST_ONLY_HEAD_SOURCE.replace(
        '#[cfg(test)]\nmod tests {',
        '#[cfg(test)]\nconst ONLY_IN_TEST: bool = true;\nmod tests {',
      ),
      rule: 'test-only-declared-module-missing',
    },
    {
      name: 'changed module header',
      head: TEST_ONLY_HEAD_SOURCE.replace(
        '#[cfg(test)]\nmod tests {',
        '#[cfg(test)]\nmod  tests {',
      ),
      rule: 'test-only-production-projection-not-proven',
    },
    {
      name: 'malformed source',
      head: `${TEST_ONLY_HEAD_SOURCE}/*`,
      rule: 'test-only-rust-source-invalid',
    },
    {
      name: 'unbalanced source',
      head: TEST_ONLY_HEAD_SOURCE.slice(0, -2),
      rule: 'test-only-rust-source-invalid',
    },
  ]

  for (const item of cases) {
    const entry = testOnlyLocalizationEntry()
    const root = testOnlyLocalizationRoot(entry, item.head)
    const result = selectCoverageEntries({ schema_version: 2, entries: [entry] }, entry.files, root, {
      baseRevision: 'e'.repeat(40),
      readBaseFile: testOnlyBaseReader(entry),
    })
    assert.equal(result.ok, false, item.name)
    assert.equal(result.errors.some((error) => error.rule === item.rule), true, item.name)
  }

  const undeclared = testOnlyLocalizationEntry()
  undeclared.verifications[0].source = 'crates/fixture/src/undeclared.rs'
  const undeclaredRoot = testOnlyLocalizationRoot(undeclared)
  const undeclaredResult = validateCoverageMap({ schema_version: 2, entries: [undeclared] }, undeclaredRoot)
  assert.equal(undeclaredResult.ok, false)
  assert.equal(undeclaredResult.errors.some((item) => item.rule === 'test-only-source-files-mismatch'), true)

  const wrongPackage = testOnlyLocalizationEntry()
  wrongPackage.verifications[0].package = 'other'
  wrongPackage.verifications[0].cargo_test = ['cargo', 'test', '-p', 'other', '--lib']
  const wrongPackageResult = validateCoverageMap(
    { schema_version: 2, entries: [wrongPackage] },
    testOnlyLocalizationRoot(wrongPackage),
  )
  assert.equal(wrongPackageResult.ok, false)
  assert.equal(wrongPackageResult.errors.some((item) => item.rule === 'test-only-verification-invalid'), true)

  const ignoredButNotTest = testOnlyLocalizationEntry()
  const ignoredButNotTestRoot = testOnlyLocalizationRoot(
    ignoredButNotTest,
    TEST_ONLY_HEAD_SOURCE.replace('    #[test]\n    #[ignore', '    #[ignore'),
  )
  const ignoredButNotTestResult = validateCoverageMap(
    { schema_version: 2, entries: [ignoredButNotTest] },
    ignoredButNotTestRoot,
  )
  assert.equal(ignoredButNotTestResult.ok, false)
  assert.equal(ignoredButNotTestResult.errors.some((item) => item.rule === 'test-only-ignored-test-missing'), true)

  const evidenceEntry = testOnlyLocalizationEntry()
  const evidenceRoot = testOnlyLocalizationRoot(evidenceEntry)
  const evidenceResult = selectCoverageEntries({ schema_version: 2, entries: [evidenceEntry] }, evidenceEntry.files, evidenceRoot, {
    baseRevision: 'f'.repeat(40),
    readBaseFile: testOnlyBaseReader(evidenceEntry, TEST_ONLY_BASE_SOURCE, ''),
  })
  assert.equal(evidenceResult.ok, false)
  assert.equal(evidenceResult.errors.some((item) => item.rule === 'test-only-ignored-evidence-missing'), true)

  const ambiguousEvidenceEntry = testOnlyLocalizationEntry()
  const ambiguousEvidenceRoot = testOnlyLocalizationRoot(ambiguousEvidenceEntry)
  const ambiguousEvidenceResult = selectCoverageEntries(
    { schema_version: 2, entries: [ambiguousEvidenceEntry] },
    ambiguousEvidenceEntry.files,
    ambiguousEvidenceRoot,
    {
      baseRevision: 'f'.repeat(40),
      readBaseFile: testOnlyBaseReader(
        ambiguousEvidenceEntry,
        TEST_ONLY_BASE_SOURCE,
        testOnlyEvidence(ambiguousEvidenceEntry).replace('**PASS**.', '**PASS**-not-terminal'),
      ),
    },
  )
  assert.equal(ambiguousEvidenceResult.ok, false)
  assert.equal(ambiguousEvidenceResult.errors.some((item) => item.rule === 'test-only-ignored-evidence-missing'), true)
})

test('no_rust_change_is_explicit_no_change_not_skip', () => {
  const root = fixtureRoot('```bash\nnode tools/ci/check-rust-slice-coverage.mjs -p fixture --files crates/fixture/src/policy.rs --min 80\n```\n')
  const result = selectCoverageEntries(coverageMap(), ['README.md'], root)
  assert.equal(result.ok, true)
  assert.equal(result.state, 'NO_CHANGE')
  assert.deepEqual(result.entries, [])
})

test('coverage_runner_uses_no_shell_and_fails_on_command_error', () => {
  const calls = []
  const entry = coverageMap().entries[0]
  const failed = runCoveragePlan([entry], {
    root: '/safe/root',
    spawn(command, args, options) {
      calls.push({ command, args, options })
      return { status: 1 }
    },
  })
  assert.equal(failed.ok, false)
  assert.equal(failed.errors.some((item) => item.rule === 'coverage-command-failed'), true)
  assert.deepEqual(calls[0], {
    command: 'node',
    args: entry.command.slice(1),
    options: { cwd: '/safe/root', shell: false, stdio: 'inherit' },
  })
})

test('coverage_map_refuses_malformed_entries_and_unsafe_changed_paths', () => {
  const root = fixtureRoot('```bash\nnode tools/ci/check-rust-slice-coverage.mjs -p fixture --files crates/fixture/src/policy.rs --min 80\n```\n')
  const invalid = coverageMap()
  invalid.entries.push({ ...invalid.entries[0] })
  invalid.entries[0].command = [
    'node', 'tools/ci/check-rust-slice-coverage.mjs', '--packages', 'fixture',
    '--files', 'crates/fixture/src/policy.rs', '--min', '80', '--report-json', '/unsafe.json',
  ]
  invalid.entries[1].files = ['crates/fixture/src/missing.rs']
  invalid.entries[1].packages = ['INVALID']
  invalid.entries[1].min = 79
  const validation = validateCoverageMap(invalid, root)
  assert.equal(validation.ok, false)
  for (const rule of ['coverage-entry-duplicate', 'coverage-command-invalid', 'coverage-packages-invalid', 'coverage-min-invalid', 'coverage-file-missing']) {
    assert.equal(validation.errors.some((item) => item.rule === rule), true, rule)
  }

  const unsafe = selectCoverageEntries(coverageMap(), ['../unsafe.rs'], root)
  assert.equal(unsafe.ok, false)
  assert.equal(unsafe.errors.some((item) => item.rule === 'changed-path-unsafe'), true)
  const bom = selectCoverageEntries(coverageMap(), [`${String.fromCodePoint(0xfeff)}crates/fixture/src/policy.rs`], root)
  assert.equal(bom.ok, false)
  assert.equal(bom.errors.some((item) => item.rule === 'changed-path-unsafe'), true)
  const nonArray = selectCoverageEntries(coverageMap(), null, root)
  assert.equal(nonArray.ok, false)
  assert.equal(nonArray.errors.some((item) => item.rule === 'changed-paths-invalid'), true)
  assert.equal(validateCoverageMap({ schema_version: 1, entries: [] }, root).ok, false)
})

test('planner_cli_rejects_bom_or_control_changed_paths_without_trimming', () => {
  const root = plannerRoot()
  const changedPathCases = [
    `${String.fromCodePoint(0xfeff)}crates/fixture/src/policy.rs`,
    `\tcrates/fixture/src/policy.rs`,
    `crates/fixture/${String.fromCodePoint(0x202e)}src/policy.rs`,
    `crates/fixture/src/policy.rs${String.fromCodePoint(0x0007)}`,
  ]
  for (const changedPath of changedPathCases) {
    writeFileSync(path.join(root, 'changed.txt'), `${changedPath}\n`)
    const output = []
    const errors = []
    assert.equal(main(['--changed-files', 'changed.txt'], {
      root,
      print: (line) => output.push(line),
      error: (line) => errors.push(line),
    }), 1)
    assert.equal(output.includes('RUST_SLICE_COVERAGE_STATUS=BLOCKED'), true)
    assert.equal(output.includes('RUST_SLICE_COVERAGE_STATUS=READY'), false)
    assert.equal(errors.includes('RUST_SLICE_COVERAGE_ERROR=changed-path-unsafe'), true)
  }
})

test('planner_cli_loads_exact_map_and_reports_read_and_execution_failures', () => {
  const root = plannerRoot()
  const output = []
  const errors = []
  const calls = []
  assert.equal(main(['--changed-files', 'changed.txt', '--run'], {
    root,
    print: (line) => output.push(line),
    error: (line) => errors.push(line),
    spawn(command, args, options) {
      calls.push({ command, args, options })
      return { status: 0 }
    },
  }), 0)
  assert.equal(output.includes('RUST_SLICE_COVERAGE_STATUS=READY'), true)
  assert.equal(output.includes('RUST_SLICE_COVERAGE_ENTRY=fixture-policy'), true)
  assert.equal(errors.length, 0)
  assert.equal(calls.length, 1)

  const mapReadErrors = []
  assert.equal(main(['--map', 'missing.json', '--all'], { root, print: () => {}, error: (line) => mapReadErrors.push(line) }), 1)
  assert.deepEqual(mapReadErrors, ['RUST_SLICE_COVERAGE_ERROR=coverage-map-read-failed'])

  const changedReadErrors = []
  assert.equal(main(['--changed-files', 'missing.txt'], { root, print: () => {}, error: (line) => changedReadErrors.push(line) }), 1)
  assert.deepEqual(changedReadErrors, ['RUST_SLICE_COVERAGE_ERROR=changed-paths-read-failed'])

  const executionErrors = []
  assert.equal(main(['--all', '--run'], {
    root,
    print: () => {},
    error: (line) => executionErrors.push(line),
    spawn: () => ({ status: 1 }),
  }), 1)
  assert.equal(executionErrors.includes('RUST_SLICE_COVERAGE_ERROR=coverage-command-failed'), true)
})

test('planner_cli_rejects_unknown_arguments_without_running_commands', () => {
  const output = []
  const errors = []
  assert.equal(main(['--unsafe'], {
    print: (line) => output.push(line),
    error: (line) => errors.push(line),
  }), 2)
  assert.deepEqual(output, [])
  assert.equal(errors.length, 1)
})

test('localization_comment_projector_is_conservative_for_literals_and_nested_comments', () => {
  const base = '/* Portuguese outer\n/* nested */\n*/\nconst RAW: &str = r#"// immutable"#;\nconst TEXT: &str = "/* immutable */";\n'
  const translated = '/* English outer\n/* nested */\n*/\nconst RAW: &str = r#"// immutable"#;\nconst TEXT: &str = "/* immutable */";\n'
  assert.equal(isCommentOnlyRustDifferential(base, translated), true)
  assert.equal(stripRustComments(base).includes('r#"// immutable"#'), true)
  assert.equal(stripRustComments(base).includes('"/* immutable */"'), true)
  assert.equal(isCommentOnlyRustDifferential(base, translated.replace('immutable"#', 'changed"#')), false)
  assert.equal(isCommentOnlyRustDifferential(base, translated.replace('const TEXT', 'const  TEXT')), false)
  assert.equal(stripRustComments('/* unclosed'), null)
  assert.equal(stripRustComments('const VALUE: &str = "unclosed'), null)
})

test('ownership_contracts_fail_closed_for_invalid_shapes_and_base_reads', () => {
  const root = fixtureRoot('```bash\nnode tools/ci/check-rust-slice-coverage.mjs -p fixture --files crates/fixture/src/policy.rs --min 80 --report-json tmp/fixture.json\n```\n')
  const lineMap = coverageMap()
  lineMap.entries[0].command.push('--report-json', 'tmp/fixture.json')
  assert.equal(validateCoverageMap(lineMap, root).ok, true)

  const invalidKind = coverageMap()
  invalidKind.entries[0].kind = 'generic-exemption'
  const invalidKindResult = validateCoverageMap(invalidKind, root)
  assert.equal(invalidKindResult.errors.some((item) => item.rule === 'coverage-kind-invalid'), true)

  const extraLineField = coverageMap()
  extraLineField.entries[0].unexpected = true
  const extraLineResult = validateCoverageMap(extraLineField, root)
  assert.equal(extraLineResult.errors.some((item) => item.rule === 'coverage-entry-fields-invalid'), true)

  const platform = platformEntry()
  const platformMap = { schema_version: 2, entries: [platform] }
  const platformRootPath = platformRoot(platform)
  platform.verifications[0].static.test = 'missing_static_test'
  const platformResult = validateCoverageMap(platformMap, platformRootPath)
  assert.equal(platformResult.errors.some((item) => item.rule === 'platform-static-test-missing'), true)
  assert.equal(platformResult.errors.some((item) => item.rule === 'platform-spec-contract-mismatch'), true)

  const localization = localizationEntry()
  const localizationMap = { schema_version: 2, entries: [localization] }
  const localizationRootPath = localizationRoot(localization)
  const unreadableBase = selectCoverageEntries(localizationMap, localization.files, localizationRootPath, {
    baseRevision: 'c'.repeat(40),
    readBaseFile() {
      return null
    },
  })
  assert.equal(unreadableBase.ok, false)
  assert.equal(unreadableBase.errors.some((item) => item.rule === 'localization-differential-base-read-failed'), true)

  const invalidBaseOutput = []
  const invalidBaseErrors = []
  assert.equal(main(['--all', '--base-revision', 'not-a-sha'], {
    root,
    print: (line) => invalidBaseOutput.push(line),
    error: (line) => invalidBaseErrors.push(line),
  }), 2)
  assert.deepEqual(invalidBaseOutput, [])
  assert.equal(invalidBaseErrors.length, 1)

  const skippedCalls = []
  assert.equal(runCoveragePlan([{ ...platformEntry() }], {
    spawn(...args) {
      skippedCalls.push(args)
      return { status: 1 }
    },
  }).ok, true)
  assert.deepEqual(skippedCalls, [])
})
