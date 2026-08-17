//! Unit tests for [`super`](status_bar.rs): the mapping from "what's
//! running" to "what the bar says", exercised against hand-built
//! [`BackgroundWork`] snapshots.
//!
//! Nothing here spawns a real language server, installer, or `git` — that's
//! the whole point of `BackgroundWork` being a plain struct: every branch
//! the bar can take is reachable without one. That the snapshot itself
//! matches reality is `BackgroundWork::gather`'s job, and it's a straight
//! read of accessors each owning subsystem already tests.
//!
//! Expected text is asserted in pt-BR, the primary language and the one
//! every test build stays on (`FoxGardenApp::new`) — for the same reason
//! `e2e::menus` gives: the active language is process-global, so a test
//! that switched it would switch it under every other test running in
//! parallel. That each string exists in both languages is a compile error
//! if it doesn't (`fg_i18n::catalog`), and that switching works at all is
//! covered in `fg_i18n`'s own test binary.

use super::*;

fn texts(work: &BackgroundWork) -> Vec<String> {
    activities(work).iter().map(Activity::text).collect()
}

#[test]
fn an_idle_app_has_no_activities_at_all() {
    assert_eq!(activities(&BackgroundWork::default()), vec![]);
}

#[test]
fn a_starting_language_server_is_named() {
    let work = BackgroundWork { starting_servers: vec!["JDTLS"], ..BackgroundWork::default() };
    assert_eq!(texts(&work), vec!["Iniciando JDTLS…"]);
}

/// An install's own progress line is the reason installs report more than
/// a bare name: jdt.ls is built from source and takes minutes, so an
/// unchanging "Instalando JDTLS…" would look indistinguishable from a hung
/// job for the whole of it.
#[test]
fn an_installs_progress_line_is_appended_to_its_label() {
    let work =
        BackgroundWork { installing: vec![("JDTLS", Some("Building…".to_string()))], ..BackgroundWork::default() };
    assert_eq!(texts(&work), vec!["Instalando JDTLS… Building…"]);
}

#[test]
fn an_install_with_no_progress_line_is_just_its_label() {
    let work = BackgroundWork { installing: vec![("PMD", None)], ..BackgroundWork::default() };
    assert_eq!(texts(&work), vec!["Instalando PMD…"]);
}

#[test]
fn every_flag_produces_its_own_line() {
    let work = BackgroundWork {
        running_checkstyle: true,
        running_pmd: true,
        scanning_classpath: true,
        running_git: true,
        detecting_java_home: true,
        checking_versions: vec!["Checkstyle"],
        ..BackgroundWork::default()
    };
    assert_eq!(
        texts(&work),
        vec![
            "Executando Checkstyle…",
            "Executando PMD…",
            "Lendo o classpath do projeto…",
            "Consultando o git…",
            "Verificando atualizações de Checkstyle…",
            "Procurando um JDK…",
        ]
    );
}

/// The bar names exactly one job, so the order decides *which* — a server
/// the editor's completions and diagnostics are waiting on outranks a
/// version check that changes nothing anyone is waiting for.
#[test]
fn a_starting_server_outranks_every_other_running_job() {
    let work = BackgroundWork {
        starting_servers: vec!["Kotlin Language Server"],
        checking_versions: vec!["PMD"],
        detecting_java_home: true,
        running_git: true,
        ..BackgroundWork::default()
    };
    assert_eq!(activities(&work)[0].text(), "Iniciando Kotlin Language Server…");
}

/// One string, read by both the Tools menu's own running-scan label and
/// the bar — the reason it lives in `Strings::common` rather than being
/// spelled out twice.
#[test]
fn the_checkstyle_line_is_worded_exactly_as_the_tools_menu_words_it() {
    let work = BackgroundWork { running_checkstyle: true, ..BackgroundWork::default() };
    assert_eq!(texts(&work), vec![t().common.running_checkstyle]);
}

/// Both servers can be coming up at once (a project with Java *and* Kotlin
/// files open), and both must be reported — the bar names the first and
/// counts the rest, but it can only do that if `activities` produced them
/// all.
#[test]
fn two_servers_starting_at_once_are_both_reported() {
    let work =
        BackgroundWork { starting_servers: vec!["JDTLS", "Kotlin Language Server"], ..BackgroundWork::default() };
    assert_eq!(texts(&work), vec!["Iniciando JDTLS…", "Iniciando Kotlin Language Server…"]);
}
