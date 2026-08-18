//! Messages that interpolate a runtime value — a path, a file name, an
//! underlying error.
//!
//! These can't be `&'static str`, so they're functions returning `String`
//! rather than fields on [`crate::Strings`]. Everything fixed lives in the
//! catalogue; the split is exactly "does this have a hole in it".
//!
//! Each message is declared with the [`msg!`] macro, which keeps the two
//! languages' wordings on adjacent lines — the only reliable way to notice
//! that one of them has drifted:
//!
//! ```
//! # use fg_i18n::{Lang, set_lang, msg};
//! set_lang(Lang::PtBr);
//! assert_eq!(msg::failed_to_save("disco cheio"), "falha ao salvar: disco cheio");
//! ```

/// Declares one interpolating message, in both languages.
///
/// The format strings use inline captures (`{err}`), so they name the
/// function's own parameters directly and a typo in either language is a
/// compile error rather than a mangled string at runtime.
macro_rules! msg {
    (
        $(#[$meta:meta])*
        $name:ident($($arg:ident: $ty:ty),* $(,)?) {
            pt: $pt:literal,
            en: $en:literal $(,)?
        }
    ) => {
        $(#[$meta])*
        pub fn $name($($arg: $ty),*) -> String {
            match $crate::lang() {
                $crate::Lang::PtBr => format!($pt),
                $crate::Lang::EnUs => format!($en),
            }
        }
    };
}

// --- Help > About -------------------------------------------------------

msg! {
    /// The app version line in the About dialog.
    version(version: &str) {
        pt: "Versão {version}",
        en: "Version {version}",
    }
}

// --- Opening, saving, and closing files ---------------------------------

msg! {
    /// The unsaved-changes prompt raised when a dirty tab is closed.
    save_changes_before_closing(name: &str) {
        pt: "Salvar as alterações em {name} antes de fechar?",
        en: "Save changes to {name} before closing?",
    }
}

msg! {
    failed_to_save(err: &str) {
        pt: "falha ao salvar: {err}",
        en: "failed to save: {err}",
    }
}

msg! {
    /// Refused because the file isn't UTF-8 text — a `.class`, a `.jar`, an
    /// image. Distinct from [`couldnt_open`], which is a real I/O failure.
    couldnt_open_not_text(path: &str) {
        pt: "Não foi possível abrir {path}: não é um arquivo de texto.",
        en: "Couldn't open {path}: not a text file.",
    }
}

msg! {
    couldnt_open(path: &str, err: &str) {
        pt: "Não foi possível abrir {path}:\n{err}",
        en: "Couldn't open {path}:\n{err}",
    }
}

msg! {
    failed_to_open_project(err: &str) {
        pt: "falha ao abrir o projeto: {err}",
        en: "failed to open project: {err}",
    }
}

msg! {
    failed_to_reopen_last_project(err: &str) {
        pt: "falha ao reabrir o último projeto: {err}",
        en: "failed to reopen last project: {err}",
    }
}

msg! {
    failed_to_reopen_tab(err: &str) {
        pt: "falha ao reabrir a aba: {err}",
        en: "failed to reopen tab: {err}",
    }
}

// --- The file watcher's "changed underneath you" banner -----------------

msg! {
    file_deleted_on_disk(name: &str) {
        pt: "⚠ {name} foi excluído no disco.",
        en: "⚠ {name} was deleted on disk.",
    }
}

msg! {
    file_changed_on_disk(name: &str) {
        pt: "⚠ {name} mudou no disco desde que você o abriu.",
        en: "⚠ {name} changed on disk since you opened it.",
    }
}

// --- The project tree ---------------------------------------------------

msg! {
    /// The New File row's prompt, naming the directory the file lands in.
    new_file_in(dir: &str) {
        pt: "Novo arquivo em {dir}:",
        en: "New file in {dir}:",
    }
}

msg! {
    file_already_exists(path: &str) {
        pt: "o arquivo já existe: {path}",
        en: "file already exists: {path}",
    }
}

msg! {
    failed_to_create_file(err: &str) {
        pt: "falha ao criar o arquivo: {err}",
        en: "failed to create file: {err}",
    }
}

msg! {
    rename_target_exists(path: &str) {
        pt: "falha ao renomear: {path} já existe",
        en: "rename failed: {path} already exists",
    }
}

msg! {
    failed_to_rename(err: &str) {
        pt: "falha ao renomear: {err}",
        en: "failed to rename: {err}",
    }
}

msg! {
    /// `failures` is an already-joined, newline-separated list.
    failed_to_paste(failures: &str) {
        pt: "falha ao colar:\n{failures}",
        en: "failed to paste:\n{failures}",
    }
}

msg! {
    /// `failures` is an already-joined, newline-separated list.
    failed_to_delete(failures: &str) {
        pt: "falha ao excluir:\n{failures}",
        en: "failed to delete:\n{failures}",
    }
}

msg! {
    confirm_delete_directory(name: &str) {
        pt: "Excluir o diretório {name} e tudo que há dentro dele? Isso não pode ser desfeito.",
        en: "Delete directory {name} and everything inside it? This cannot be undone.",
    }
}

msg! {
    confirm_delete_file(name: &str) {
        pt: "Excluir {name}? Isso não pode ser desfeito.",
        en: "Delete {name}? This cannot be undone.",
    }
}

msg! {
    /// Deliberately not pluralised: this branch is only reached with two or
    /// more selected items, so "1 item" can never occur.
    confirm_delete_many(count: usize) {
        pt: "Excluir {count} itens? Isso não pode ser desfeito.",
        en: "Delete {count} items? This cannot be undone.",
    }
}

msg! {
    failed_to_refresh_tree(err: &str) {
        pt: "falha ao atualizar a árvore do projeto: {err}",
        en: "failed to refresh project tree: {err}",
    }
}

// --- Terminal -----------------------------------------------------------

msg! {
    failed_to_open_terminal(err: &str) {
        pt: "falha ao abrir o terminal: {err}",
        en: "failed to open terminal: {err}",
    }
}

msg! {
    failed_to_start_terminal(err: &str) {
        pt: "falha ao iniciar o terminal: {err}",
        en: "failed to start terminal: {err}",
    }
}

// --- git ----------------------------------------------------------------

msg! {
    git_status_failed(err: &str) {
        pt: "git status falhou: {err}",
        en: "git status failed: {err}",
    }
}

msg! {
    git_diff_failed(err: &str) {
        pt: "git diff falhou: {err}",
        en: "git diff failed: {err}",
    }
}

msg! {
    git_operation_failed(err: &str) {
        pt: "a operação do git falhou: {err}",
        en: "Git operation failed: {err}",
    }
}

// --- External tools and language servers --------------------------------

msg! {
    checkstyle_failed(err: &str) {
        pt: "o Checkstyle falhou: {err}",
        en: "Checkstyle failed: {err}",
    }
}

msg! {
    /// A build's own `Command::spawn` failing (`mvn`/`gradle` missing from
    /// `PATH`, e.g.) — not the build itself failing, which instead shows up
    /// as ordinary compiler-error rows in the build output panel.
    failed_to_start_build(err: &str) {
        pt: "falha ao iniciar a compilação: {err}",
        en: "failed to start build: {err}",
    }
}

msg! {
    pmd_failed(err: &str) {
        pt: "o PMD falhou: {err}",
        en: "PMD failed: {err}",
    }
}

msg! {
    install_failed(err: &str) {
        pt: "a instalação falhou: {err}",
        en: "Install failed: {err}",
    }
}

msg! {
    language_server_install_failed(err: &str) {
        pt: "a instalação do servidor de linguagem falhou: {err}",
        en: "Language server install failed: {err}",
    }
}

msg! {
    installed_version(version: &str) {
        pt: "Instalado: {version}",
        en: "Installed: {version}",
    }
}

msg! {
    up_to_date_at(version: &str) {
        pt: "Atualizado ({version})",
        en: "Up to date ({version})",
    }
}

msg! {
    githubs_latest(version: &str) {
        pt: "Mais recente no GitHub: {version}",
        en: "GitHub's latest: {version}",
    }
}

msg! {
    update_check_failed(err: &str) {
        pt: "a verificação de atualizações falhou: {err}",
        en: "Update check failed: {err}",
    }
}

msg! {
    /// Hover text on Install/Reinstall, explaining why the pinned version
    /// is what gets installed even when GitHub has something newer.
    install_pin_explanation(pinned: &str) {
        pt: "Instalar sempre usa a versão {pinned}, que este app verificou que roda corretamente — \
             não necessariamente a mais nova no GitHub. Um lançamento mais recente ali é esperado, \
             não um problema; Reinstalar ainda vai instalar {pinned}, a menos que uma atualização \
             futura do FoxGarden mude esse pin.",
        en: "Install always uses {pinned}, the version this app has verified actually runs \
             correctly — not necessarily whatever's newest on GitHub. A newer release here is \
             expected, not a problem; Reinstall will still install {pinned} unless a future \
             FoxGarden update changes that pin.",
    }
}

msg! {
    installs_from(name: &str, version: &str, repo: &str) {
        pt: "Instala {name} {version}, a partir de {repo}",
        en: "Installs {name} {version}, from {repo}",
    }
}

msg! {
    available_upstream(version: &str) {
        pt: "{version} disponível no upstream (ainda não incluída)",
        en: "{version} available upstream (not yet bundled)",
    }
}

// --- The status bar -----------------------------------------------------

msg! {
    /// A language server whose `initialize` handshake is still in flight —
    /// `name` is the server's own product name (`JDTLS`), never translated.
    starting_language_server(name: &str) {
        pt: "Iniciando {name}…",
        en: "Starting {name}…",
    }
}

msg! {
    /// A background install of a language server or an external tool.
    /// [`crate::Strings::install`]'s own `installing` is the dialog's
    /// button-adjacent "Installing…" with nothing to name; this one names
    /// what's being installed, since the status bar has no surrounding
    /// dialog to say which.
    installing_named(name: &str) {
        pt: "Instalando {name}…",
        en: "Installing {name}…",
    }
}

msg! {
    checking_for_updates_to(name: &str) {
        pt: "Verificando atualizações de {name}…",
        en: "Checking {name} for updates…",
    }
}

// --- Generate / Override Method -----------------------------------------

msg! {
    no_superclass(class_name: &str) {
        pt: "{class_name} não tem superclasse nem interface de onde sobrescrever métodos.",
        en: "{class_name} has no superclass or interface to override methods from.",
    }
}

// --- File > New Project… -------------------------------------------------

msg! {
    /// The wizard's own preview line, shown once Location/Artifact ID are
    /// both filled in — states exactly where Create will scaffold to,
    /// before the user commits to it.
    will_create_project(path: &str) {
        pt: "Será criado em: {path}",
        en: "Will create: {path}",
    }
}

msg! {
    scaffold_failed(err: &str) {
        pt: "falha ao gerar o projeto: {err}",
        en: "failed to scaffold the project: {err}",
    }
}

msg! {
    scaffolded_but_config_save_failed(root: &str, err: &str) {
        pt: "{root} foi gerado, mas não foi possível salvar sua configuração de projeto: {err}",
        en: "scaffolded {root} but couldn't save its project config: {err}",
    }
}

msg! {
    scaffolded_but_open_failed(root: &str, err: &str) {
        pt: "{root} foi gerado, mas não foi possível abri-lo: {err}",
        en: "scaffolded {root} but couldn't open it: {err}",
    }
}

msg! {
    superclass_not_in_project(super_name: &str) {
        pt: "Sobrescrever Método só procura superclasses dentro deste projeto \
             (não foi possível encontrar {super_name}.java).",
        en: "Override Method only looks up superclasses in this project \
             (couldn't find {super_name}.java).",
    }
}

msg! {
    failed_to_read(path: &str, err: &str) {
        pt: "falha ao ler {path}: {err}",
        en: "failed to read {path}: {err}",
    }
}

msg! {
    no_overridable_methods(super_name: &str) {
        pt: "Nenhum método sobrescrevível encontrado em {super_name} \
             (ou todos já foram sobrescritos).",
        en: "No overridable methods found on {super_name} (or they're all already overridden).",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Lang, with_lang};

    #[test]
    fn interpolates_in_both_languages() {
        with_lang(Lang::PtBr, || {
            assert_eq!(failed_to_save("disco cheio"), "falha ao salvar: disco cheio");
        });
        with_lang(Lang::EnUs, || {
            assert_eq!(failed_to_save("disk full"), "failed to save: disk full");
        });
    }

    #[test]
    fn a_message_with_two_holes_fills_both() {
        with_lang(Lang::EnUs, || {
            assert_eq!(
                failed_to_read("/tmp/a.java", "no such file"),
                "failed to read /tmp/a.java: no such file"
            );
        });
    }

    /// `install_pin_explanation` names the pinned version twice in both
    /// languages — a rewording that drops one of them would leave a
    /// dangling "will still install" with nothing after it.
    #[test]
    fn a_repeated_hole_is_filled_every_time() {
        for lang in Lang::ALL {
            with_lang(lang, || {
                assert_eq!(
                    install_pin_explanation("10.20.1").matches("10.20.1").count(),
                    2,
                    "{lang:?}"
                );
            });
        }
    }
}
