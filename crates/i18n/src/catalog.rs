//! The string catalogue itself: [`Strings`] and one `const` per language.
//!
//! Grouped into one sub-struct per UI area, so a call site reads as
//! `t().menu.file` / `t().git.commit` rather than as a flat namespace of a
//! few hundred fields. Strings shared by more than one area (a `Close`
//! button, a `Cancel`) live in [`Common`] instead of being repeated per
//! area — the alternative drifts, and a `Cancelar` next to a `Cancelar!` in
//! two dialogs is exactly the kind of thing a catalogue exists to prevent.
//!
//! Product and protocol nouns are deliberately *not* translated: `Java`,
//! `Kotlin`, `Spring`, `Checkstyle`, `PMD`, `commit`, `push`, `blame`,
//! `getter`/`setter`, `JAVA_HOME`. These are what a Brazilian Java developer
//! actually says and searches for; translating them would make the UI
//! harder to use, not easier.

/// Every user-visible fixed string in the app, for one language.
pub struct Strings {
    pub menu: Menu,
    pub dialogs: Dialogs,
    pub about: About,
    pub side_panel: SidePanel,
    pub tabs: Tabs,
    pub welcome: Welcome,
    pub file_history: FileHistory,
    pub git: Git,
    pub lsp: Lsp,
    pub external_tools: ExternalTools,
    pub run_configs: RunConfigs,
    pub new_project: NewProject,
    pub palettes: Palettes,
    pub editor: Editor,
    pub codegen: Codegen,
    pub install: Install,
    pub status_bar: StatusBar,
    pub errors: Errors,
    pub common: Common,
    pub debug: Debug,
    pub jdk: Jdk,
}

/// The menu bar: `File`, `Settings`, `Tools`, `Run`, `View`, `Help`, plus
/// the side-panel toggle pinned to its right edge.
pub struct Menu {
    pub file: &'static str,
    pub new_file: &'static str,
    pub open_folder: &'static str,
    pub new_project: &'static str,
    pub close_tab: &'static str,
    pub reopen_closed_tab: &'static str,
    pub exit: &'static str,

    pub settings: &'static str,
    pub theme: &'static str,
    pub theme_light: &'static str,
    pub theme_dark: &'static str,
    pub font: &'static str,
    pub indentation: &'static str,
    pub indent_spaces: &'static str,
    pub indent_tabs: &'static str,
    pub indent_width: &'static str,
    pub auto_save: &'static str,
    pub auto_save_enabled: &'static str,
    pub auto_save_on_focus_loss: &'static str,
    pub auto_save_after_idle: &'static str,
    pub auto_save_idle_seconds: &'static str,
    pub trim_trailing_whitespace: &'static str,
    pub trim_trailing_whitespace_hint: &'static str,
    pub language: &'static str,
    pub language_servers: &'static str,
    pub jdks: &'static str,
    pub external_tools: &'static str,

    pub tools: &'static str,
    pub read_only: &'static str,
    pub generate_getters: &'static str,
    pub generate_setters: &'static str,
    pub generate_constructor: &'static str,
    pub generate_to_string: &'static str,
    pub generate_equals_and_hash_code: &'static str,
    pub override_method: &'static str,
    pub convert_to_uppercase: &'static str,
    pub convert_to_lowercase: &'static str,
    pub convert_to_title_case: &'static str,
    pub sort_lines: &'static str,
    pub unique_lines: &'static str,
    pub run_checkstyle: &'static str,
    pub run_pmd: &'static str,
    pub run_spotbugs: &'static str,

    pub run: &'static str,
    pub edit_configurations: &'static str,
    pub build: &'static str,
    pub run_project: &'static str,
    pub run_tests: &'static str,
    pub run_with_coverage: &'static str,
    pub docker_build_and_run: &'static str,
    pub docker_compose_up: &'static str,
    pub debug_project: &'static str,
    pub profile: &'static str,

    pub view: &'static str,
    pub zen_mode: &'static str,
    pub side_panel: &'static str,
    pub terminal_panel: &'static str,
    pub source_control: &'static str,
    pub build_output: &'static str,
    pub profiler_panel: &'static str,
    pub word_wrap: &'static str,
    pub render_whitespace: &'static str,
    pub indentation_guides: &'static str,
    pub sticky_scroll: &'static str,
    pub sticky_scroll_hint: &'static str,
    pub blinking_cursor: &'static str,
    pub editor_outline: &'static str,
    pub editor_outline_hint: &'static str,
    pub inline_blame: &'static str,
    pub inline_blame_hint: &'static str,
    pub fold_all: &'static str,
    pub expand_all: &'static str,

    pub help: &'static str,
    pub live_templates: &'static str,
    pub about: &'static str,

    pub collapse_side_panel: &'static str,
    pub expand_side_panel: &'static str,
}

/// Settings > Font… and Help > Live Templates….
pub struct Dialogs {
    pub font_heading: &'static str,
    pub font_size: &'static str,

    pub live_templates_heading: &'static str,
    pub live_templates_intro: &'static str,
    pub live_templates_custom_intro: &'static str,
    pub live_templates_global: &'static str,
    pub live_templates_global_hint: &'static str,
    pub live_templates_yours: &'static str,
    pub live_templates_trigger_hint: &'static str,
    pub live_templates_expansion_hint: &'static str,
}

/// Help > About. The product name itself ("FoxGarden") is not a translated
/// string, so it isn't here.
pub struct About {
    pub author: &'static str,
    pub shortcuts_heading: &'static str,
    /// The shortcut reference lines, in display order. A fixed-size array
    /// rather than a slice so a language that forgets a line fails to
    /// compile instead of silently showing a shorter list.
    pub shortcuts: [&'static str; 19],
}

/// The project tree and its toolbar.
pub struct SidePanel {
    pub open_folder_hint: &'static str,
    pub new_file_hint: &'static str,
    pub open_terminal_hint: &'static str,
    pub create: &'static str,
    pub no_folder_open: &'static str,
}

/// The tab bar and its close-with-unsaved-changes prompt.
/// The empty-state screen shown when no file is open.
pub struct Welcome {
    pub tagline: &'static str,
    pub recent_projects: &'static str,
    pub hint_go_to_file: &'static str,
    pub hint_recent_files: &'static str,
    pub hint_command_palette: &'static str,
    pub hint_terminal: &'static str,
}

pub struct Tabs {
    pub no_file_open: &'static str,
    pub allow_editing: &'static str,
    pub discard: &'static str,
    pub close: &'static str,
    pub close_others: &'static str,
    pub close_to_the_right: &'static str,
    pub copy_path: &'static str,
    pub reveal_in_tree: &'static str,
    /// Overflow indicator on the tab bar: how many tabs are scrolled out of
    /// view, e.g. "4 mais".
    pub more_tabs: &'static str,
    pub more_tabs_hint: &'static str,
    pub save_all: &'static str,
    /// Shown in a split editor pane that has no tab active in it.
    pub empty_pane: &'static str,
    /// View-menu items for the split editor (`PLAN.md` Track 11).
    pub split_editor: &'static str,
    pub unsplit_editor: &'static str,
}

/// The tab context menu's "File History…" window (`PLAN.md` Track 4 Phase
/// 2): every `.foxgarden/history/` snapshot for one file, diffed against
/// its own currently open (possibly unsaved) buffer.
pub struct FileHistory {
    pub menu_item: &'static str,
    pub heading_prefix: &'static str,
    pub loading: &'static str,
    pub empty: &'static str,
    pub revert: &'static str,
    pub side_by_side: &'static str,
    pub inline: &'static str,
}

/// The Source Control panel. `commit`, `push`, `stage` and `hunk` stay in
/// English: they're git's own vocabulary, which is what the CLI, the docs
/// and every Brazilian developer's conversation already use.
pub struct Git {
    pub heading: &'static str,
    pub refresh: &'static str,
    pub push: &'static str,
    pub commit: &'static str,
    pub commit_message_hint: &'static str,
    pub loading: &'static str,
    pub loading_hunks: &'static str,
    pub no_changes: &'static str,
    pub hunks: &'static str,
    pub diff: &'static str,
    pub stage_hunk: &'static str,
    pub unstage_hunk: &'static str,
    pub side_by_side: &'static str,
    pub inline: &'static str,
}

/// Settings > Language Servers….
pub struct Lsp {
    pub heading: &'static str,
    pub up_to_date: &'static str,
    pub binary: &'static str,
    /// Placeholder in the empty binary-path field — points at what to do
    /// when nothing is configured yet.
    pub binary_hint: &'static str,
    pub java_home: &'static str,
    /// Hover on the Java Home label — why it exists and what blank means.
    pub java_home_hover: &'static str,
    pub detect: &'static str,
    pub detect_hint: &'static str,
    pub project_java: &'static str,
    /// Hover on the "Project Java" label — where the release is read from.
    pub project_java_hover: &'static str,
    pub project_java_undeclared: &'static str,
}

/// Settings > JDKs… (`panels::jdk_registry`) — the machine-wide inventory
/// of JDKs a project can target. `JDKs` stays in English: it's an acronym,
/// identical in both languages, and matches its own menu entry.
pub struct Jdk {
    pub hint: &'static str,
    pub none_registered: &'static str,
    pub add: &'static str,
    pub auto_detect: &'static str,
}

/// Settings > External Tools… (Checkstyle, PMD, SpotBugs).
pub struct ExternalTools {
    pub heading: &'static str,
}

/// Run > Edit Configurations….
pub struct RunConfigs {
    pub heading: &'static str,
    pub new: &'static str,
    pub duplicate: &'static str,
    pub name: &'static str,
    /// Placeholder examples shown in each empty field — a hint at the
    /// expected format, never a value that gets submitted.
    pub name_hint: &'static str,
    pub main_class: &'static str,
    pub main_class_hint: &'static str,
    pub vm_args: &'static str,
    pub vm_args_hint: &'static str,
    pub program_args: &'static str,
    pub program_args_hint: &'static str,
    pub working_dir: &'static str,
    pub working_dir_hint: &'static str,
    pub browse: &'static str,
    pub env_vars: &'static str,
    pub env_key_hint: &'static str,
    pub env_value_hint: &'static str,
    pub add: &'static str,
    pub empty_hint: &'static str,
}

/// File > New Project… — `PLAN.md` Track 29 Phase 3. `create`/`cancel`'s
/// generic sibling lives in [`Common`] (`t().common.cancel`) since Cancel
/// is worded identically everywhere in this app; `create` isn't, since no
/// other dialog has a matching terminal action to share the word with.
pub struct NewProject {
    pub heading: &'static str,
    pub description: &'static str,
    pub group_id: &'static str,
    /// Placeholder shown in the empty Group ID field — an example Maven
    /// coordinate, not a default that gets submitted.
    pub group_id_hint: &'static str,
    pub artifact_id: &'static str,
    pub artifact_id_hint: &'static str,
    pub location: &'static str,
    pub location_hint: &'static str,
    pub browse: &'static str,
    pub java_release: &'static str,
    pub build_tool: &'static str,
    pub build_tool_maven: &'static str,
    pub build_tool_gradle: &'static str,
    pub language: &'static str,
    pub language_java: &'static str,
    pub language_kotlin: &'static str,
    pub gradle_no_wrapper_hint: &'static str,
    pub create: &'static str,
}

/// The `Ctrl+P`/`Ctrl+E`/`Ctrl+Shift+E` popups and the terminal panel's
/// tab strip.
pub struct Palettes {
    /// The command palette's own prompt, and the name of the one command
    /// that has no menu item to borrow wording from.
    pub run_a_command: &'static str,
    pub next_diagnostic: &'static str,
    pub go_to_file: &'static str,
    pub go_to_recent_file: &'static str,
    pub spring_endpoints: &'static str,
    pub new_terminal: &'static str,
    pub scanning_project: &'static str,
    pub no_terminal_session: &'static str,
}

/// The editor's right-click context menu.
pub struct Editor {
    pub undo: &'static str,
    pub redo: &'static str,
    pub select_all: &'static str,
    pub toggle_line_comment: &'static str,
    pub duplicate_line: &'static str,
}

/// The Generate/Override pickers the Tools menu opens over the editor.
pub struct Codegen {
    pub generate_for: &'static str,
    pub generate_accessors_for: &'static str,
    pub override_for: &'static str,
    pub generate: &'static str,
}

/// The install/update row shared by Settings > Language Servers… and
/// Settings > External Tools….
pub struct Install {
    pub install: &'static str,
    pub reinstall: &'static str,
    pub installing: &'static str,
    pub checking: &'static str,
    pub check_for_updates: &'static str,
}

/// The bottom status bar's own fixed labels — the jobs it reports that
/// name nothing at runtime. The ones that name a language server or a tool
/// (`Iniciando JDTLS…`) interpolate that name, so they live in
/// [`crate::msg`] instead.
pub struct StatusBar {
    /// The language indicator's text for a file with no recognized
    /// language — it still opens and edits, it just has no highlighting.
    pub plain_text: &'static str,
    /// Hover text over the error/warning counts.
    pub diagnostics_hint: &'static str,
    /// Shown when nothing at all is running, so the bar keeps its height
    /// (and its meaning: "the app is not busy", not "the bar is broken").
    pub ready: &'static str,
    pub detecting_java_home: &'static str,
    pub scanning_classpath: &'static str,
    pub running_git: &'static str,
}

/// Error messages that interpolate nothing, so they can stay `&'static
/// str`. The ones that carry a path or an underlying error live in
/// [`crate::msg`] instead.
pub struct Errors {
    pub select_text_first: &'static str,
    pub accessors_java_only: &'static str,
    pub no_class_fields: &'static str,
    pub every_field_is_final: &'static str,
    pub accessors_no_tree: &'static str,
    pub generate_java_only: &'static str,
    pub generate_no_tree: &'static str,
    pub override_java_only: &'static str,
    pub override_needs_class: &'static str,
    pub override_no_tree: &'static str,
    pub checkstyle_not_configured: &'static str,
    pub pmd_not_configured: &'static str,
    pub spotbugs_not_configured: &'static str,
    pub spotbugs_no_compiled_classes: &'static str,
    pub no_build_tool_detected: &'static str,
    pub no_run_config: &'static str,
    pub coverage_requires_maven: &'static str,
    pub no_dockerfile_detected: &'static str,
    pub no_compose_file_detected: &'static str,
    pub rename_empty_name: &'static str,
    pub rename_no_parent: &'static str,
}

/// Strings used by more than one area — button verbs, mostly.
pub struct Common {
    pub restore: &'static str,
    /// Clears every toast at once.
    pub dismiss_all: &'static str,
    pub ok: &'static str,
    pub close: &'static str,
    pub cancel: &'static str,
    pub save: &'static str,
    pub delete: &'static str,
    pub copy: &'static str,
    pub cut: &'static str,
    pub paste: &'static str,
    pub rename: &'static str,
    pub new_file: &'static str,
    pub remove: &'static str,
    pub add: &'static str,
    pub dismiss: &'static str,
    pub reload: &'static str,
    pub keep_mine: &'static str,
    pub not_installed: &'static str,
    pub no_matches: &'static str,
    /// The Tools menu shows this in place of `menu.run_checkstyle` while a
    /// scan is in flight, and the status bar reports the same scan with the
    /// same words — one string, so the two can't drift apart.
    pub running_checkstyle: &'static str,
    pub running_pmd: &'static str,
    pub running_spotbugs: &'static str,
    /// The Run menu shows this in place of `menu.build` while a build is in
    /// flight, and the status bar reports the same build with the same
    /// words — same "one string, so the two can't drift apart" shape as
    /// `running_checkstyle`/`running_pmd`.
    pub running_build: &'static str,
    pub running_run: &'static str,
    pub running_tests: &'static str,
    pub running_coverage: &'static str,
    /// The Run menu shows this in place of `menu.docker_build_and_run`
    /// while `docker build`/`docker run` is in flight (`PLAN.md` Track 14
    /// Phase 1).
    pub running_docker_build: &'static str,
    /// Same as `running_docker_build`, for `menu.docker_compose_up` while
    /// `docker compose up --build` is in flight.
    pub running_docker_compose: &'static str,
    /// The Run menu shows this in place of `menu.debug_project` once a
    /// debug session is starting or attached (`PLAN.md` Track 23 Phase 1),
    /// same "one string, so the button and any status line can't drift
    /// apart" shape every other `running_*` entry here already follows.
    pub running_debug: &'static str,
    /// The Run menu shows this in place of `menu.profile` while an
    /// async-profiler capture is sampling the launched run (`PLAN.md` Track
    /// 26 Phase 1).
    pub profiling: &'static str,
    /// The profiler panel's own title and states (`PLAN.md` Track 26 Phase 2).
    pub profiler_title: &'static str,
    pub profiler_no_profile: &'static str,
    pub profiler_reset_zoom: &'static str,
    pub profiler_hint: &'static str,
    /// Stops a currently-running Run (`PLAN.md` Track 22 Phase 2) — a
    /// generic enough verb to belong here rather than under `Menu`/
    /// `RunConfigs`, matching this struct's own "shared by more than one
    /// area" purpose. Now also `PLAN.md` Track 23's own Stop-a-debug-
    /// session control, per this field's own prior doc comment predicting
    /// exactly that reuse.
    pub stop: &'static str,
    /// Debug toolbar buttons (`PLAN.md` Track 23 Phase 2) — only enabled
    /// while a debug session is actually paused at a breakpoint.
    pub debug_continue: &'static str,
    pub debug_step_over: &'static str,
    pub debug_step_into: &'static str,
    pub debug_step_out: &'static str,
}

/// The debugger's variables/call-stack panel (`debug_panel`), shown only
/// while a session is live.
pub struct Debug {
    /// Shown while a session is running but not stopped at a breakpoint.
    pub not_paused: &'static str,
    pub call_stack: &'static str,
    pub variables: &'static str,
    /// Placeholder while the adapter's variable scopes haven't arrived yet.
    pub loading: &'static str,
    /// A scope that resolved to no variables.
    pub none: &'static str,
}

/// Brazilian Portuguese — the primary language.
pub const PT_BR: Strings = Strings {
    menu: Menu {
        file: "Arquivo",
        new_file: "Novo Arquivo…",
        open_folder: "Abrir Pasta…",
        new_project: "Novo Projeto…",
        close_tab: "Fechar Aba",
        reopen_closed_tab: "Reabrir Aba Fechada",
        exit: "Sair",

        settings: "Configurações",
        theme: "Tema",
        theme_light: "Claro",
        theme_dark: "Escuro",
        font: "Fonte…",
        indentation: "Indentação",
        indent_spaces: "Espaços",
        indent_tabs: "Tabulações",
        indent_width: "Largura",
        auto_save: "Salvamento Automático",
        auto_save_enabled: "Ativado",
        auto_save_on_focus_loss: "Ao Perder o Foco",
        auto_save_after_idle: "Após Inatividade",
        auto_save_idle_seconds: "Segundos de Inatividade",
        trim_trailing_whitespace: "Remover Espaços ao Salvar",
        trim_trailing_whitespace_hint: "Remove espaços e tabulações no fim de cada linha ao salvar. Desligue para não gerar diferenças em linhas que você não editou.",
        language: "Idioma",
        language_servers: "Servidores de Linguagem…",
        jdks: "JDKs…",
        external_tools: "Ferramentas Externas…",

        tools: "Ferramentas",
        read_only: "Somente Leitura",
        generate_getters: "Gerar Getters",
        generate_setters: "Gerar Setters",
        generate_constructor: "Gerar Construtor",
        generate_to_string: "Gerar toString()",
        generate_equals_and_hash_code: "Gerar equals() e hashCode()",
        override_method: "Sobrescrever Método",
        convert_to_uppercase: "Converter para MAIÚSCULAS",
        convert_to_lowercase: "Converter para minúsculas",
        convert_to_title_case: "Converter para Iniciais Maiúsculas",
        sort_lines: "Ordenar Linhas",
        unique_lines: "Remover Linhas Duplicadas",
        run_checkstyle: "Executar Checkstyle",
        run_pmd: "Executar PMD",
        run_spotbugs: "Executar SpotBugs",

        run: "Executar",
        edit_configurations: "Editar Configurações…",
        build: "Compilar",
        run_project: "Executar Projeto",
        run_tests: "Executar Testes",
        run_with_coverage: "Executar com Cobertura",
        docker_build_and_run: "Docker: Construir e Executar",
        docker_compose_up: "Docker Compose: Subir",
        debug_project: "Depurar Projeto",
        profile: "Analisar Processo em Execução",

        view: "Exibir",
        zen_mode: "Modo Zen",
        side_panel: "Painel Lateral",
        terminal_panel: "Painel do Terminal",
        source_control: "Controle de Versão",
        build_output: "Saída da Compilação",
        profiler_panel: "Painel de Perfil",
        word_wrap: "Quebra Automática de Linha",
        render_whitespace: "Exibir Espaços em Branco",
        indentation_guides: "Guias de Indentação",
        sticky_scroll: "Rolagem Fixa",
        sticky_scroll_hint: "Fixa o cabeçalho da classe/método atual durante a rolagem (Java)",
        blinking_cursor: "Cursor Piscante",
        editor_outline: "Contorno do Editor",
        editor_outline_hint: "Borda ao redor do painel de edição ativo, destacada enquanto ele tem o foco",
        inline_blame: "Blame na Linha",
        inline_blame_hint: "Anotação esmaecida de autor/data/resumo na linha atual do cursor",
        fold_all: "Recolher Tudo",
        expand_all: "Expandir Tudo",

        help: "Ajuda",
        live_templates: "Modelos Dinâmicos…",
        about: "Sobre",

        collapse_side_panel: "Recolher Painel Lateral (Ctrl+B)",
        expand_side_panel: "Expandir Painel Lateral (Ctrl+B)",
    },
    dialogs: Dialogs {
        font_heading: "Fonte",
        font_size: "Tamanho",

        live_templates_heading: "Modelos Dinâmicos",
        live_templates_intro: "Digite um gatilho abaixo e pressione Tab sem seleção para expandi-lo.",
        live_templates_custom_intro: "Adicione os seus abaixo — um gatilho seu substitui um embutido de mesmo nome.",
        live_templates_global: "Global",
        live_templates_global_hint: "Expande da mesma forma em qualquer arquivo, Java/Kotlin ou não.",
        live_templates_yours: "Seus Modelos",
        live_templates_trigger_hint: "gatilho",
        live_templates_expansion_hint: "expansão — ${cursor} marca onde o cursor para",
    },
    about: About {
        author: "Por Rômulo Fernandes Evangelista",
        shortcuts_heading: "Atalhos:",
        shortcuts: [
            "Ctrl+S — salvar a aba ativa",
            "Ctrl+Shift+T — reabrir a última aba fechada",
            "Clique do meio numa aba — fechá-la",
            "F11 — alternar o Modo Zen (esconde menu e painel lateral)",
            "Ctrl+B — alternar o painel lateral",
            "Ctrl+J — juntar a linha atual com a seguinte",
            "Ctrl+E — ir para um arquivo recente",
            "Ctrl+Shift+E — buscar endpoints do Spring",
            "Ctrl+/ — comentar/descomentar linhas",
            "Ctrl+Shift+G — gerar getters e setters (Java)",
            "Ctrl+Shift+U/L — converter a seleção para MAIÚSCULAS/minúsculas",
            "Menu Ferramentas — gerar só getters/setters, ou Iniciais Maiúsculas",
            "Digite um gatilho de snippet (ex.: \"sout\") e Tab para expandi-lo",
            "Ajuda > Modelos Dinâmicos… — lista completa de gatilhos",
            "Alt+Up/Down — mover a linha atual para cima/baixo",
            "Alt+Shift+Up/Down — duplicar a linha atual",
            "Home — ir ao primeiro caractere não-branco, depois à coluna 0",
            "Ctrl+N — novo arquivo",
            "Esc — fechar o diálogo atual",
        ],
    },
    side_panel: SidePanel {
        open_folder_hint: "Abrir Pasta",
        new_file_hint: "Novo Arquivo",
        open_terminal_hint: "Abrir Terminal",
        create: "Criar",
        no_folder_open: "Nenhuma pasta aberta",
    },
    welcome: Welcome {
        tagline: "Editor leve para Java e Kotlin",
        recent_projects: "Projetos recentes",
        hint_go_to_file: "Ctrl+P: ir para arquivo",
        hint_recent_files: "Ctrl+E: arquivos recentes",
        hint_command_palette: "Ctrl+Shift+P: comandos",
        hint_terminal: "Ctrl+`: terminal",
    },
    tabs: Tabs {
        no_file_open: "Nenhum arquivo aberto",
        allow_editing: "Permitir Edição",
        discard: "Descartar",
        close: "Fechar",
        close_others: "Fechar as Outras",
        close_to_the_right: "Fechar as da Direita",
        copy_path: "Copiar Caminho",
        reveal_in_tree: "Mostrar na Árvore",
        more_tabs: "mais",
        more_tabs_hint: "Abas fora da vista — role a barra ou use Ctrl+E",
        save_all: "Salvar Todos",
        empty_pane: "Nenhuma aba neste painel — clique em uma aba para abri-la aqui.",
        split_editor: "Dividir Editor",
        unsplit_editor: "Desfazer Divisão",
    },
    file_history: FileHistory {
        menu_item: "Histórico do Arquivo…",
        heading_prefix: "Histórico: ",
        loading: "Carregando…",
        empty: "Nenhum snapshot ainda",
        revert: "Reverter para Esta Versão",
        side_by_side: "Lado a Lado",
        inline: "Em Linha",
    },
    git: Git {
        heading: "Controle de Versão",
        refresh: "Atualizar",
        push: "Push",
        commit: "Commit",
        commit_message_hint: "Mensagem do commit",
        loading: "Carregando…",
        loading_hunks: "Carregando hunks…",
        no_changes: "Nenhuma alteração",
        hunks: "Hunks",
        diff: "Diff",
        stage_hunk: "Stage do Hunk",
        unstage_hunk: "Unstage do Hunk",
        side_by_side: "Lado a Lado",
        inline: "Em Linha",
    },
    lsp: Lsp {
        heading: "Servidores de Linguagem",
        up_to_date: "Atualizado",
        binary: "Binário",
        binary_hint: "caminho do executável, ou use Instalar",
        java_home: "Java Home",
        java_home_hover: "O jdt.ls precisa de um JDK 21+ para rodar. Preenchido automaticamente com o JDK 21+ mais novo \
                          encontrado nesta máquina; edite para apontar para outro (ex.: \
                          ~/.sdkman/candidates/java/21.0.11-zulu). Vazio usa JAVA_HOME/PATH.",
        detect: "Detectar",
        detect_hint: "Procura um JDK 21 ou mais novo em JAVA_HOME, no PATH e nos diretórios de instalação usuais.",
        project_java: "Java do Projeto",
        project_java_hover: "Lido dos arquivos de build do projeto (pom.xml, build.gradle, .java-version). Fontes Java e \
                             Kotlin são diagnosticadas neste release, usando o JDK correspondente da lista encontrada \
                             nesta máquina.",
        project_java_undeclared: "não declarado — o jdt.ls usa o release da própria JVM",
    },
    external_tools: ExternalTools {
        heading: "Ferramentas Externas",
    },
    run_configs: RunConfigs {
        heading: "Configurações de Execução",
        new: "+ Nova",
        duplicate: "Duplicar",
        name: "Nome",
        name_hint: "Minha Configuração",
        main_class: "Classe Principal",
        main_class_hint: "com.exemplo.Main",
        vm_args: "Argumentos da VM",
        vm_args_hint: "-Xmx512m -ea",
        program_args: "Argumentos do Programa",
        program_args_hint: "--verbose entrada.txt",
        working_dir: "Diretório de Trabalho",
        working_dir_hint: "raiz do projeto",
        browse: "Procurar…",
        env_vars: "Variáveis de Ambiente",
        env_key_hint: "CHAVE",
        env_value_hint: "valor",
        add: "Adicionar",
        empty_hint: "Nenhuma configuração de execução ainda — clique em \"+ Nova\" para adicionar uma.",
    },
    new_project: NewProject {
        heading: "Novo Projeto",
        description: "Cria um novo projeto Java ou Kotlin e o abre.",
        group_id: "Group ID",
        group_id_hint: "com.exemplo",
        artifact_id: "Artifact ID",
        artifact_id_hint: "minha-app",
        location: "Local",
        location_hint: "~/projetos",
        browse: "Procurar…",
        java_release: "Versão do Java",
        build_tool: "Ferramenta de Build",
        build_tool_maven: "Maven",
        build_tool_gradle: "Gradle (Kotlin DSL)",
        language: "Linguagem",
        language_java: "Java",
        language_kotlin: "Kotlin",
        gradle_no_wrapper_hint: "Nenhum Gradle Wrapper é gerado — use um Gradle já instalado na máquina.",
        create: "Criar",
    },
    palettes: Palettes {
        run_a_command: "Executar comando",
        next_diagnostic: "Próximo problema",
        go_to_file: "Ir para arquivo",
        go_to_recent_file: "Ir para arquivo recente",
        spring_endpoints: "Endpoints do Spring",
        new_terminal: "Novo Terminal",
        scanning_project: "Analisando o projeto…",
        no_terminal_session: "Nenhuma sessão de terminal",
    },
    editor: Editor {
        undo: "Desfazer",
        redo: "Refazer",
        select_all: "Selecionar Tudo",
        toggle_line_comment: "Comentar/Descomentar Linha",
        duplicate_line: "Duplicar Linha",
    },
    codegen: Codegen {
        generate_for: "Gerar para:",
        generate_accessors_for: "Gerar acessores para:",
        override_for: "Sobrescrever:",
        generate: "Gerar",
    },
    install: Install {
        install: "Instalar",
        reinstall: "Reinstalar",
        installing: "Instalando…",
        checking: "Verificando…",
        check_for_updates: "Verificar Atualizações",
    },
    status_bar: StatusBar {
        plain_text: "Texto",
        diagnostics_hint: "Erros e avisos neste arquivo",
        ready: "Pronto",
        detecting_java_home: "Procurando um JDK…",
        scanning_classpath: "Lendo o classpath do projeto…",
        running_git: "Consultando o git…",
    },
    errors: Errors {
        select_text_first: "Selecione algum texto primeiro e tente de novo.",
        accessors_java_only: "Gerar getters/setters só funciona em arquivos Java.",
        no_class_fields: "Nenhum campo de classe encontrado neste arquivo.",
        every_field_is_final: "Nada a gerar: todos os campos aqui são final.",
        accessors_no_tree: "Não foi possível gerar os acessores: ainda não há árvore sintática.",
        generate_java_only: "Gerar Construtor/toString/equals() só funciona em arquivos Java.",
        generate_no_tree: "Não foi possível gerar: ainda não há árvore sintática.",
        override_java_only: "Sobrescrever Método só funciona em arquivos Java.",
        override_needs_class: "Posicione o cursor dentro de uma classe para sobrescrever um método.",
        override_no_tree: "Não foi possível procurar métodos sobrescrevíveis: ainda não há árvore sintática.",
        checkstyle_not_configured: "Defina o binário e o caminho de configuração do Checkstyle em Configurações > Ferramentas Externas primeiro.",
        no_build_tool_detected: "Nenhum pom.xml ou build.gradle[.kts] encontrado na raiz do projeto.",
        no_run_config: "Crie uma configuração de execução primeiro, em Executar > Editar Configurações…",
        coverage_requires_maven: "Cobertura de código só é suportada em projetos Maven no momento.",
        no_dockerfile_detected: "Nenhum Dockerfile encontrado na raiz do projeto.",
        no_compose_file_detected: "Nenhum arquivo compose.yaml/docker-compose.yml encontrado na raiz do projeto.",
        pmd_not_configured: "Defina o binário e o caminho do ruleset do PMD em Configurações > Ferramentas Externas primeiro.",
        spotbugs_not_configured: "Defina o binário do SpotBugs em Configurações > Ferramentas Externas primeiro.",
        spotbugs_no_compiled_classes: "Nenhuma classe compilada encontrada. Execute Executar > Compilar primeiro.",
        rename_empty_name: "falha ao renomear: nome vazio",
        rename_no_parent: "falha ao renomear: sem diretório pai",
    },
    common: Common {
        dismiss_all: "Dispensar tudo",
        restore: "Restaurar",
        ok: "OK",
        close: "Fechar",
        cancel: "Cancelar",
        save: "Salvar",
        delete: "Excluir",
        copy: "Copiar",
        cut: "Recortar",
        paste: "Colar",
        rename: "Renomear",
        new_file: "Novo Arquivo",
        remove: "Remover",
        add: "+ Adicionar",
        dismiss: "Dispensar",
        reload: "Recarregar",
        keep_mine: "Manter o Meu",
        not_installed: "Não instalado",
        no_matches: "Nenhum resultado",
        running_checkstyle: "Executando Checkstyle…",
        running_pmd: "Executando PMD…",
        running_spotbugs: "Executando SpotBugs…",
        running_build: "Compilando…",
        running_run: "Executando…",
        running_tests: "Executando Testes…",
        running_coverage: "Executando Cobertura…",
        running_docker_build: "Construindo e Executando Docker…",
        running_docker_compose: "Subindo Docker Compose…",
        running_debug: "Depurando…",
        profiling: "Analisando…",
        profiler_title: "Perfil (Flame Graph)",
        profiler_no_profile: "Nenhum perfil capturado ainda. Execute um projeto e use Executar › Analisar Processo em Execução.",
        profiler_reset_zoom: "Reduzir Zoom",
        profiler_hint: "Clique num quadro para dar zoom; clique no topo para reduzir.",
        stop: "Parar",
        debug_continue: "Continuar",
        debug_step_over: "Passar Por Cima",
        debug_step_into: "Entrar Em",
        debug_step_out: "Sair De",
    },
    debug: Debug {
        not_paused: "Não pausado.",
        call_stack: "Pilha de Chamadas",
        variables: "Variáveis",
        loading: "(carregando…)",
        none: "(nenhuma)",
    },
    jdk: Jdk {
        hint: "Os JDKs registrados aqui ficam disponíveis para usar como alvo ao analisar ou criar um projeto em \
               uma versão específica do Java — separado do Java Home em Configurações > Servidores de Linguagem…, \
               que é apenas a JVM sob a qual o próprio jdt.ls roda.",
        none_registered: "Nenhum JDK registrado ainda.",
        add: "Adicionar JDK…",
        auto_detect: "Detectar automaticamente",
    },
};

/// US English.
pub const EN_US: Strings = Strings {
    menu: Menu {
        file: "File",
        new_file: "New File…",
        open_folder: "Open Folder…",
        new_project: "New Project…",
        close_tab: "Close Tab",
        reopen_closed_tab: "Reopen Closed Tab",
        exit: "Exit",

        settings: "Settings",
        theme: "Theme",
        theme_light: "Light",
        theme_dark: "Dark",
        font: "Font…",
        indentation: "Indentation",
        indent_spaces: "Spaces",
        indent_tabs: "Tabs",
        indent_width: "Width",
        auto_save: "Auto-save",
        auto_save_enabled: "Enabled",
        auto_save_on_focus_loss: "On Focus Loss",
        auto_save_after_idle: "After Idle",
        auto_save_idle_seconds: "Idle Seconds",
        trim_trailing_whitespace: "Trim Trailing Whitespace on Save",
        trim_trailing_whitespace_hint: "Strips spaces and tabs at the end of every line when saving. Turn off to avoid touching lines you didn't edit.",
        language: "Language",
        language_servers: "Language Servers…",
        jdks: "JDKs…",
        external_tools: "External Tools…",

        tools: "Tools",
        read_only: "Read-Only",
        generate_getters: "Generate Getters",
        generate_setters: "Generate Setters",
        generate_constructor: "Generate Constructor",
        generate_to_string: "Generate toString()",
        generate_equals_and_hash_code: "Generate equals() and hashCode()",
        override_method: "Override Method",
        convert_to_uppercase: "Convert to UPPERCASE",
        convert_to_lowercase: "Convert to lowercase",
        convert_to_title_case: "Convert to Title Case",
        sort_lines: "Sort Lines",
        unique_lines: "Unique Lines",
        run_checkstyle: "Run Checkstyle",
        run_pmd: "Run PMD",
        run_spotbugs: "Run SpotBugs",

        run: "Run",
        edit_configurations: "Edit Configurations…",
        build: "Build",
        run_project: "Run Project",
        run_tests: "Run Tests",
        run_with_coverage: "Run with Coverage",
        docker_build_and_run: "Docker: Build & Run",
        docker_compose_up: "Docker Compose: Up",
        debug_project: "Debug Project",
        profile: "Profile Running Process",

        view: "View",
        zen_mode: "Zen Mode",
        side_panel: "Side Panel",
        terminal_panel: "Terminal Panel",
        source_control: "Source Control",
        build_output: "Build Output",
        profiler_panel: "Profiler Panel",
        word_wrap: "Word Wrap",
        render_whitespace: "Render Whitespace",
        indentation_guides: "Indentation Guides",
        sticky_scroll: "Sticky Scroll",
        sticky_scroll_hint: "Pin the enclosing class/method header while scrolling (Java)",
        blinking_cursor: "Blinking Cursor",
        editor_outline: "Editor Outline",
        editor_outline_hint: "Border around the active editor pane, highlighted while it has focus",
        inline_blame: "Inline Blame",
        inline_blame_hint: "Dimmed author/date/summary annotation on the cursor's current line",
        fold_all: "Fold All",
        expand_all: "Expand All",

        help: "Help",
        live_templates: "Live Templates…",
        about: "About",

        collapse_side_panel: "Collapse Side Panel (Ctrl+B)",
        expand_side_panel: "Expand Side Panel (Ctrl+B)",
    },
    dialogs: Dialogs {
        font_heading: "Font",
        font_size: "Size",

        live_templates_heading: "Live Templates",
        live_templates_intro: "Type a trigger below, then press Tab with no selection to expand it.",
        live_templates_custom_intro: "Add your own below — a custom trigger overrides a built-in one of the same name.",
        live_templates_global: "Global",
        live_templates_global_hint: "Expands the same way in every file, Java/Kotlin or not.",
        live_templates_yours: "Your Templates",
        live_templates_trigger_hint: "trigger",
        live_templates_expansion_hint: "expansion — ${cursor} marks where the cursor lands",
    },
    about: About {
        author: "By Rômulo Fernandes Evangelista",
        shortcuts_heading: "Shortcuts:",
        shortcuts: [
            "Ctrl+S — save the active tab",
            "Ctrl+Shift+T — reopen the last closed tab",
            "Middle-click a tab — close it",
            "F11 — toggle Zen Mode (hide menu bar and side panel)",
            "Ctrl+B — toggle the side panel",
            "Ctrl+J — join the current line with the next one",
            "Ctrl+E — go to a recent file",
            "Ctrl+Shift+E — search Spring endpoints",
            "Ctrl+/ — toggle line comments",
            "Ctrl+Shift+G — generate getters and setters (Java)",
            "Ctrl+Shift+U/L — convert selection to UPPER/lowercase",
            "Tools menu — generate just getters/setters, or Title Case",
            "Type a snippet trigger (e.g. \"sout\") then Tab to expand it",
            "Help > Live Templates… — full list of snippet triggers",
            "Alt+Up/Down — move the current line up/down",
            "Alt+Shift+Up/Down — duplicate the current line",
            "Home — jump to first non-whitespace, then column 0",
            "Ctrl+N — new file",
            "Esc — close the current dialog",
        ],
    },
    side_panel: SidePanel {
        open_folder_hint: "Open Folder",
        new_file_hint: "New File",
        open_terminal_hint: "Open Terminal",
        create: "Create",
        no_folder_open: "No folder open",
    },
    welcome: Welcome {
        tagline: "A light editor for Java and Kotlin",
        recent_projects: "Recent projects",
        hint_go_to_file: "Ctrl+P: go to file",
        hint_recent_files: "Ctrl+E: recent files",
        hint_command_palette: "Ctrl+Shift+P: commands",
        hint_terminal: "Ctrl+`: terminal",
    },
    tabs: Tabs {
        no_file_open: "No file open",
        allow_editing: "Allow Editing",
        discard: "Discard",
        close: "Close",
        close_others: "Close Others",
        close_to_the_right: "Close to the Right",
        copy_path: "Copy Path",
        reveal_in_tree: "Reveal in Tree",
        more_tabs: "more",
        more_tabs_hint: "Tabs scrolled out of view — scroll the bar, or use Ctrl+E",
        save_all: "Save All",
        empty_pane: "No tab in this pane — click a tab to open it here.",
        split_editor: "Split Editor",
        unsplit_editor: "Unsplit Editor",
    },
    file_history: FileHistory {
        menu_item: "File History…",
        heading_prefix: "History: ",
        loading: "Loading…",
        empty: "No snapshots yet",
        revert: "Revert to This Version",
        side_by_side: "Side by Side",
        inline: "Inline",
    },
    git: Git {
        heading: "Source Control",
        refresh: "Refresh",
        push: "Push",
        commit: "Commit",
        commit_message_hint: "Commit message",
        loading: "Loading…",
        loading_hunks: "Loading hunks…",
        no_changes: "No changes",
        hunks: "Hunks",
        diff: "Diff",
        stage_hunk: "Stage Hunk",
        unstage_hunk: "Unstage Hunk",
        side_by_side: "Side by Side",
        inline: "Inline",
    },
    lsp: Lsp {
        heading: "Language Servers",
        up_to_date: "Up to date",
        binary: "Binary",
        binary_hint: "path to the executable, or use Install",
        java_home: "Java Home",
        java_home_hover: "jdt.ls requires a JDK 21+ to run. Filled in automatically from the newest JDK 21+ found on \
                          this machine; edit it to point at a different one (e.g. ~/.sdkman/candidates/java/21.0.11-zulu). \
                          Blank falls back to JAVA_HOME/PATH.",
        detect: "Detect",
        detect_hint: "Scans JAVA_HOME, PATH and the usual JDK install directories for a JDK 21 or newer.",
        project_java: "Project Java",
        project_java_hover: "Read from the project's own build files (pom.xml, build.gradle, .java-version). Java and \
                             Kotlin sources are diagnosed at this release, using the matching JDK from the list found \
                             on this machine.",
        project_java_undeclared: "not declared — jdt.ls uses its own JVM's release",
    },
    external_tools: ExternalTools {
        heading: "External Tools",
    },
    run_configs: RunConfigs {
        heading: "Run Configurations",
        new: "+ New",
        duplicate: "Duplicate",
        name: "Name",
        name_hint: "My Configuration",
        main_class: "Main Class",
        main_class_hint: "com.example.Main",
        vm_args: "VM Args",
        vm_args_hint: "-Xmx512m -ea",
        program_args: "Program Args",
        program_args_hint: "--verbose input.txt",
        working_dir: "Working Dir",
        working_dir_hint: "project root",
        browse: "Browse…",
        env_vars: "Environment Variables",
        env_key_hint: "KEY",
        env_value_hint: "value",
        add: "Add",
        empty_hint: "No run configurations yet — click \"+ New\" to add one.",
    },
    new_project: NewProject {
        heading: "New Project",
        description: "Creates a new Java or Kotlin project and opens it.",
        group_id: "Group ID",
        group_id_hint: "com.example",
        artifact_id: "Artifact ID",
        artifact_id_hint: "my-app",
        location: "Location",
        location_hint: "~/projects",
        browse: "Browse…",
        java_release: "Java release",
        build_tool: "Build Tool",
        build_tool_maven: "Maven",
        build_tool_gradle: "Gradle (Kotlin DSL)",
        language: "Language",
        language_java: "Java",
        language_kotlin: "Kotlin",
        gradle_no_wrapper_hint: "No Gradle Wrapper is generated — use a Gradle already installed on your machine.",
        create: "Create",
    },
    palettes: Palettes {
        run_a_command: "Run a command",
        next_diagnostic: "Next problem",
        go_to_file: "Go to file",
        go_to_recent_file: "Go to recent file",
        spring_endpoints: "Spring endpoints",
        new_terminal: "New Terminal",
        scanning_project: "Scanning project…",
        no_terminal_session: "No terminal session",
    },
    editor: Editor {
        undo: "Undo",
        redo: "Redo",
        select_all: "Select All",
        toggle_line_comment: "Toggle Line Comment",
        duplicate_line: "Duplicate Line",
    },
    codegen: Codegen {
        generate_for: "Generate for:",
        generate_accessors_for: "Generate accessors for:",
        override_for: "Override:",
        generate: "Generate",
    },
    install: Install {
        install: "Install",
        reinstall: "Reinstall",
        installing: "Installing…",
        checking: "Checking…",
        check_for_updates: "Check for Updates",
    },
    status_bar: StatusBar {
        plain_text: "Plain Text",
        diagnostics_hint: "Errors and warnings in this file",
        ready: "Ready",
        detecting_java_home: "Looking for a JDK…",
        scanning_classpath: "Reading the project classpath…",
        running_git: "Running git…",
    },
    errors: Errors {
        select_text_first: "Select some text first, then try again.",
        accessors_java_only: "Generate getters/setters only works for Java files.",
        no_class_fields: "No class fields found in this file.",
        every_field_is_final: "Nothing to generate: every field here is final.",
        accessors_no_tree: "Couldn't generate accessors: no syntax tree available yet.",
        generate_java_only: "Generate Constructor/toString/equals() only works for Java files.",
        generate_no_tree: "Couldn't generate: no syntax tree available yet.",
        override_java_only: "Override Method only works for Java files.",
        override_needs_class: "Place the cursor inside a class to override a method.",
        override_no_tree: "Couldn't find overridable methods: no syntax tree available yet.",
        checkstyle_not_configured: "Set the Checkstyle binary and config path in Settings > External Tools first.",
        no_build_tool_detected: "No pom.xml or build.gradle[.kts] found at the project root.",
        no_run_config: "Create a Run Configuration first, under Run > Edit Configurations…",
        coverage_requires_maven: "Code coverage is only supported for Maven projects right now.",
        no_dockerfile_detected: "No Dockerfile found at the project root.",
        no_compose_file_detected: "No compose.yaml/docker-compose.yml found at the project root.",
        pmd_not_configured: "Set the PMD binary and ruleset path in Settings > External Tools first.",
        spotbugs_not_configured: "Set the SpotBugs binary in Settings > External Tools first.",
        spotbugs_no_compiled_classes: "No compiled classes found. Run Run > Build first.",
        rename_empty_name: "rename failed: empty name",
        rename_no_parent: "rename failed: no parent directory",
    },
    common: Common {
        dismiss_all: "Dismiss all",
        restore: "Restore",
        ok: "OK",
        close: "Close",
        cancel: "Cancel",
        save: "Save",
        delete: "Delete",
        copy: "Copy",
        cut: "Cut",
        paste: "Paste",
        rename: "Rename",
        new_file: "New File",
        remove: "Remove",
        add: "+ Add",
        dismiss: "Dismiss",
        reload: "Reload",
        keep_mine: "Keep Mine",
        not_installed: "Not installed",
        no_matches: "No matches",
        running_checkstyle: "Running Checkstyle…",
        running_pmd: "Running PMD…",
        running_spotbugs: "Running SpotBugs…",
        running_build: "Building…",
        running_run: "Running…",
        running_tests: "Running Tests…",
        running_coverage: "Running Coverage…",
        running_docker_build: "Building & Running Docker…",
        running_docker_compose: "Bringing Up Docker Compose…",
        running_debug: "Debugging…",
        profiling: "Profiling…",
        profiler_title: "Profile (Flame Graph)",
        profiler_no_profile: "No profile captured yet. Run a project, then use Run › Profile Running Process.",
        profiler_reset_zoom: "Reset Zoom",
        profiler_hint: "Click a frame to zoom in; click the top frame to zoom out.",
        stop: "Stop",
        debug_continue: "Continue",
        debug_step_over: "Step Over",
        debug_step_into: "Step Into",
        debug_step_out: "Step Out",
    },
    debug: Debug {
        not_paused: "Not paused.",
        call_stack: "Call Stack",
        variables: "Variables",
        loading: "(loading…)",
        none: "(none)",
    },
    jdk: Jdk {
        hint: "JDKs registered here are available to target when analyzing or scaffolding a project at a \
               specific Java version — separate from Settings > Language Servers…'s Java Home, which is only \
               the JVM jdt.ls itself runs under.",
        none_registered: "No JDKs registered yet.",
        add: "Add JDK…",
        auto_detect: "Auto-detect",
    },
};
