use std::io::{self, Write};

use super::doctor_types::{
    ConfigCheck, DirCheck, DoctorReport, HarnessCheck, LinkCheck, MemoryDbCheck, ProfileHandshake,
    ServerCheck, SkillsLibraryCheck,
};

pub(super) fn human(w: &mut dyn Write, report: &DoctorReport) -> io::Result<()> {
    writeln!(w, "symbrain doctor")?;
    writeln!(w)?;
    dir(w, "config dir", &report.config_dir)?;
    dir(w, "data dir", &report.data_dir)?;
    dir(w, "cache dir", &report.cache_dir)?;
    dir(w, "managed dir", &report.managed_dir)?;
    config(w, &report.config)?;
    writeln!(w)?;
    for builtin in &report.builtins {
        writeln!(w, "  ✓  {builtin:<8} built in")?;
    }
    for server in &report.servers {
        server_line(w, server)?;
    }
    memory_db(w, &report.memory_db)?;
    skills_library(w, &report.skills_library)?;
    if !report.managed_cores.is_empty() {
        writeln!(w, "  managed cores:")?;
        for core in &report.managed_cores {
            if core.version.is_empty() {
                writeln!(
                    w,
                    "    →  {:<10} not installed (pinned {}) — run `symbrain doctor --fix`",
                    core.name, core.pinned
                )?;
            } else {
                writeln!(
                    w,
                    "    ✓  {:<10} {} (pinned {})",
                    core.name, core.version, core.pinned
                )?;
            }
        }
    }
    writeln!(w)?;
    if report.profiles.is_empty() {
        writeln!(
            w,
            "  →  no profiles found (run `symbrain init` for examples)"
        )?;
    } else {
        writeln!(w, "  ✓  profiles: {}", report.profiles.join(", "))?;
    }
    writeln!(w)?;
    for harness in &report.harnesses {
        harness_line(w, harness)?;
    }
    if !report.handshakes.is_empty() {
        writeln!(w, "\n  profile handshakes:")?;
        for handshake in &report.handshakes {
            handshake_line(w, handshake)?;
        }
    }
    if !report.links.is_empty() {
        writeln!(w, "\n  links:")?;
        for link in &report.links {
            link_line(w, link)?;
        }
    }
    if !report.foreign_access_risks.is_empty() {
        writeln!(w, "\n  foreign server access risks:")?;
        for risk in &report.foreign_access_risks {
            writeln!(
                w,
                "    !  {}: {:<14} {}",
                risk.profile, risk.server, risk.detail
            )?;
        }
    }
    if !report.degradations.is_empty() {
        writeln!(w, "\n  startup degradations:")?;
        for degradation in &report.degradations {
            writeln!(
                w,
                "    !  {:<8} {}: {}",
                degradation.server, degradation.level, degradation.reason
            )?;
        }
    }
    Ok(())
}

fn dir(w: &mut dyn Write, label: &str, check: &DirCheck) -> io::Result<()> {
    writeln!(
        w,
        "  {}  {label:<12} {}",
        if check.exists { "✓" } else { "✗" },
        check.path
    )
}
fn config(w: &mut dyn Write, check: &ConfigCheck) -> io::Result<()> {
    if !check.exists {
        writeln!(
            w,
            "  →  {:<12} not found (run `symbrain init`)",
            "config.toml"
        )
    } else if check.parsed {
        writeln!(w, "  ✓  {:<12} {}", "config.toml", check.path)
    } else {
        writeln!(
            w,
            "  ✗  {:<12} {}: {}",
            "config.toml", check.path, check.error
        )
    }
}
fn server_line(w: &mut dyn Write, server: &ServerCheck) -> io::Result<()> {
    let origin = if server.origin.is_empty() {
        String::new()
    } else {
        format!(" [{}]", server.origin)
    };
    if server.found && server.probe_error.is_empty() {
        writeln!(
            w,
            "  ✓  {:<8} {} ({}){}",
            server.name, server.path, server.version, origin
        )
    } else if server.found {
        writeln!(
            w,
            "  ✗  {:<8} {} (version probe failed: {}){}",
            server.name, server.path, server.probe_error, origin
        )
    } else if !server.managed_version.is_empty() {
        writeln!(
            w,
            "  →  {:<8} managed v{} (not on PATH)",
            server.name, server.managed_version
        )
    } else {
        writeln!(
            w,
            "  →  {:<8} not found on PATH — {}",
            server.name, server.install_hint
        )
    }
}
fn memory_db(w: &mut dyn Write, check: &MemoryDbCheck) -> io::Result<()> {
    let legacy = if check.legacy {
        " (legacy symmemory install)"
    } else {
        ""
    };
    if !check.error.is_empty() {
        writeln!(
            w,
            "  ✗  {:<14} {}: {}",
            "memory db", check.path, check.error
        )
    } else if !check.exists {
        writeln!(
            w,
            "  →  {:<14} not yet created: {}",
            "memory db", check.path
        )
    } else if !check.mode_ok {
        writeln!(
            w,
            "  ✗  {:<14} {} (mode {}, want 0600){}",
            "memory db", check.path, check.mode, legacy
        )
    } else {
        writeln!(
            w,
            "  ✓  {:<14} {} (quick_check: {}){}",
            "memory db", check.path, check.quick_check, legacy
        )
    }
}
fn skills_library(w: &mut dyn Write, check: &SkillsLibraryCheck) -> io::Result<()> {
    let legacy = if check.legacy {
        " (legacy symskills install)"
    } else {
        ""
    };
    if check.error.is_empty() {
        writeln!(
            w,
            "  ✓  {:<14} {} ({} installed){}",
            "skills library", check.path, check.count, legacy
        )
    } else {
        writeln!(
            w,
            "  !  {:<14} {} ({}, installed, {}){}",
            "skills library", check.path, check.count, check.error, legacy
        )
    }
}
fn harness_line(w: &mut dyn Write, check: &HarnessCheck) -> io::Result<()> {
    if !check.config_found {
        writeln!(
            w,
            "  →  {:<14} config not found: {}",
            check.name, check.config_path
        )?;
    } else if !check.config_parsed {
        writeln!(
            w,
            "  ✗  {:<14} {} (invalid config: {})",
            check.name, check.config_path, check.config_error
        )?;
    } else if !check.installed {
        writeln!(
            w,
            "  →  {:<14} config found, symbrain not installed: {}",
            check.name, check.config_path
        )?;
    } else if check.profile.is_empty() {
        writeln!(
            w,
            "  ✗  {:<14} installed but no --profile bound: {}",
            check.name, check.config_path
        )?;
    } else if check.profile_missing {
        writeln!(
            w,
            "  ✗  {:<14} installed, bound to missing profile {:?}: {}",
            check.name, check.profile, check.config_path
        )?;
    } else {
        writeln!(
            w,
            "  ✓  {:<14} installed, profile {:?}: {}",
            check.name, check.profile, check.config_path
        )?;
    }
    if !check.superseded.is_empty() {
        let joined = check.superseded.join(", ");
        if check.installed {
            writeln!(
                w,
                "  !  {:<14} superseded core entries registered beside symbrain: {} (run `symbrain install` to migrate)",
                check.name, joined
            )?;
        } else {
            writeln!(
                w,
                "  →  {:<14} superseded core entries present (no symbrain): {}",
                check.name, joined
            )?;
        }
    }
    Ok(())
}
fn handshake_line(w: &mut dyn Write, check: &ProfileHandshake) -> io::Result<()> {
    if check.error.is_empty() {
        writeln!(
            w,
            "    ✓  {:<10} {:<8} protocol={} tools={} exposed={} hidden={} unknown={}",
            check.profile,
            check.server,
            check.protocol_version,
            check.tool_count,
            check.exposed,
            check.hidden,
            check.unknown
        )
    } else {
        writeln!(
            w,
            "    ✗  {:<10} {:<8} handshake failed: {}",
            check.profile, check.server, check.error
        )
    }
}
fn link_line(w: &mut dyn Write, check: &LinkCheck) -> io::Result<()> {
    match check.status.as_str() {
        "pass" => writeln!(w, "    ✓  {:<55} {}", check.name, check.detail),
        "unknown" => writeln!(w, "    →  {:<55} {}", check.name, check.detail),
        _ if check.remedy.is_empty() => writeln!(w, "    ✗  {:<55} {}", check.name, check.detail),
        _ => writeln!(
            w,
            "    ✗  {:<55} {} — {}",
            check.name, check.detail, check.remedy
        ),
    }
}
