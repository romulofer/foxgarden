# WIP — code review fixes (2026-09-03)

Trabalho **não commitado** na branch `ide-henshin`. Base: `df7944e`.
Suíte: `cargo test --workspace` → **1350 passando, 0 falhando** (antes: 5
falhas por Git LFS). `cargo clippy --workspace` → 8 warnings, todos
pré-existentes em código de teste.

**Não rodar `cargo fmt`**: o repositório não é rustfmt-clean por opção
(`HEAD` limpo já acusa 641 diffs). Formatação foi mantida no estilo manual
do entorno.

## Contexto

Partiu de um code review do projeto inteiro (não só do diff). Achados novos
+ itens já listados em `ISSUES.md`/`TECHNICAL_DEBT.md` que continuavam
abertos. Depois veio "corrija tudo" e, em seguida, um pedido de análise de
usabilidade (essa parte **não foi entregue** — ver Pendências).

## O que foi corrigido

### Perda de dados / correção

1. **Save atômico** — novo `crates/core/src/atomic_file.rs`
   (`write_atomically`): temp irmão + `sync_all` + `rename`, preserva
   permissões, escreve através de symlink. `Document::save` usa isso; antes
   `fs::write` truncava o arquivo antes de escrever (crash no meio = fonte
   truncado).
2. **Trim de whitespace virou opção** — `Document::save(trim: bool)`,
   Settings > "Remover Espaços ao Salvar" (padrão ligado), persistido em
   `TRIM_TRAILING_WHITESPACE_KEY`. Strings em `catalog.rs` (pt/en).
3. **`FolderPicker` em background** — novo `crates/app/src/folder_picker.rs`.
   `rfd::pick_folder()` era chamado inline em 3 lugares (side panel, File >
   Open Folder, Run Configs > Browse) e **congela a app inteira**:
   reproduzido ao vivo hoje — o diálogo vai pelo portal XDG, apareceu no
   display real em vez do Xvfb, e a UI ficou permanentemente travada
   (TECHNICAL_DEBT #23). `new_project.rs` e `jdk_registry.rs`, que já tinham
   cópias próprias do padrão, foram migrados para o helper.
4. **Erros não se sobrescrevem mais** — novo `crates/app/src/errors.rs`
   (`report`): acumula até 5 mensagens distintas em vez de a última apagar
   as anteriores. Todos os `*last_error = Some(...)` (61 sites) passaram a
   usá-lo.
5. **Erro de instalação nomeia a ferramenta** — `poll_installs` agora
   devolve `(Tool, Result)` / `(Server, Result)`; mensagens `install_failed`
   e `language_server_install_failed` receberam o parâmetro.

### Segurança

6. **sha256 pinado nos downloads** (ISSUES #8) — `tool_manager` verifica o
   artefato antes de instalar; hashes calculados a partir dos downloads
   reais nesta sessão (Checkstyle 10.26.1, PMD 7.26.0, SpotBugs 4.10.3).
   Versão sem hash pinado é recusada. Dependência nova: `sha2 = "0.10"`.

### Performance (barra Zed)

7. **Chave de cache O(1)** — novo `fg_core::TextBuffer` (`crates/core/src/
   text_buffer.rs`): `Rope` + `revision`, incrementado via `DerefMut`
   (conservador por construção; `replace()` para troca inteira).
   `Document::buffer` passou a ser `TextBuffer`. Os 5 caches por frame
   (highlight, folds, ocorrências, row counts ×2) usam a revisão em vez de
   `hash_rope_content`, que percorria o rope inteiro várias vezes por frame.
   `ContentKey` (em `text_area/cache.rs`) cobre o caso do buffer editado
   dentro do frame, onde ainda se hasheia.
8. **Highlight só da janela visível** — `syntax::highlight_spans_in` (query
   com `set_byte_range`); `widget::highlight_window` estima as linhas
   visíveis (`text_area::visible_line_window`), alarga em 200 linhas e
   arredonda para blocos de 500 (evita nova query a cada pixel de scroll).
   Fallback para documento inteiro quando há folds sem word-wrap.
9. **LSP incremental** — `lsp_state::incremental_change` diffa prefixo/
   sufixo contra o texto que o servidor realmente recebeu (`open_documents`
   virou `HashMap<PathBuf, String>`); respeita `advertised_sync_kind` do
   `initialize` (cai para full se o servidor não pedir incremental). Antes:
   documento inteiro serializado a cada tecla.
10. **`file_uri` memoizado** — `FILE_URI_CACHE`; o `canonicalize()` por
    documento a cada `publishDiagnostics` saiu do caminho quente.
11. **Diagnósticos sem clone por frame** — `paint_diagnostics` recebe
    `&[&Diagnostic]`.
12. **`History` com `VecDeque`** — `pop_front()` no lugar de `remove(0)`.
13. **`touched.binary_search`** em `auto_edit` (indent/comentário de bloco
    eram O(n·m)).
14. **`byte_to_char`/`char_to_byte`** contam lead bytes UTF-8 em vez de
    decodificar caracteres.

### Árvore de projeto / painel lateral

15. **Reage a mudanças externas** — o watcher agora observa também
    `Project::directories()` (que já exclui `target/`, `.git/`, …); um
    create/remove agenda refresh com debounce de 300 ms.
16. **Refresh em background** — o re-walk roda em thread e entra via
    `EditorState::refresh_project_tree` (ignora resultado se o projeto
    aberto mudou no meio).
17. **Patch incremental** — `Project::remove_path`/`insert_path`; criar,
    renomear, apagar e colar atualizam a árvore no mesmo frame, sem
    re-walk. O rebuild síncrono do `side_panel` foi removido.

### Manutenção

18. **`EditorRequests`** — os 5 parâmetros posicionais (`Option<
    CaseConversion>` + quatro `bool` seguidos) de `widget::show`/`tabs::show`
    viraram struct nomeada com `Default`. ~50 call sites atualizados.
19. **Testes de LFS não falham mais** — `vendored_archives_present()` faz os
    5 testes de `lsp_manager` se auto-pularem (com aviso) quando
    `vendor/lsp-servers/` só tem ponteiros LFS. O erro de runtime para o
    usuário final continua (`reject_lfs_pointer`).

## Testes novos

`atomic_file` (5), `text_buffer` (4), `errors` (4), `folder_picker` (4),
`project` (4: remove/insert/directories), `incremental_change` +
`advertised_sync_kind` (8), `verify_download` (3), `highlight_spans_in` (1),
`text_offset` (1 property-ish), `save` sem trim (1).

## Análise de usabilidade (2026-09-04)

Feita com o app real rodando em Xvfb (`:77`), projeto Maven de teste com 9
classes, navegando por cliques/teclado sintéticos. Screenshots em
`/tmp/fg-ux/*.png` (efêmero — `/tmp` é limpo entre sessões); script em
`/tmp/fg-ux/drive.py` (usa `python-xlib`).

Já corrigido nesta rodada: **terminal sem `TERM`** (commit `53a5d40`) — o
zsh imprimia `tput: No value for $TERM` e o próprio prompt como
`{nl}{i}{rst}` literal. Agora `tput colors` → 256.

### Defeitos observados, por prioridade

1. **Abas somem na borda direita, sem scroll nem indicador.** Com 9
   arquivos abertos, a barra corta em "S…" e as abas restantes ficam
   inalcançáveis pelo mouse — inclusive a **aba ativa**, que fica fora de
   vista enquanto se edita. O `*` de arquivo modificado também some junto.
   *Sugestão:* barra rolável horizontalmente + botão de overflow ("⌄ 4
   more") + auto-scroll para a aba ativa. `panels/tabs.rs` usa
   `horizontal_wrapped`; trocar por `ScrollArea::horizontal`.
2. **Ícones dependem de fonte de emoji do sistema.** `📁 📄 💻 ☕ 🔷`
   aparecem como tofu (quadrado vazio) num ambiente sem fonte de emoji —
   painel, abas e árvore inteiros ficam ilegíveis. `style/fonts.rs` embute
   JetBrains Mono e Nerd Font Symbols, mas nenhum emoji.
   *Sugestão:* usar os glifos do Nerd Font já embutido (que tem ícones de
   arquivo/pasta/terminal) em vez de emoji Unicode, ou embutir NotoEmoji.
3. **Nomes longos quebram em duas linhas na árvore.** `Application.java`
   renderiza o ícone numa linha e o nome na seguinte, desalinhando a lista.
   *Sugestão:* truncar com elipse no meio (`Applica…n.java`) e tooltip com
   o nome completo; ou scroll horizontal no painel.
4. **Barra de status não diz nada sobre o arquivo.** Só reporta trabalho de
   fundo ("Ready"). Sem linha/coluna, linguagem, indentação, encoding, nem
   contagem de erros.
   *Sugestão:* seção à direita com `Ln 11, Col 20 · Java · Spaces: 4 · UTF-8`
   e um contador de diagnósticos clicável.
5. **Erros só existem como modal bloqueante.** Uma falha de fundo (install,
   LSP, git) interrompe a digitação com um diálogo que precisa ser
   dispensado.
   *Sugestão:* toast não-modal com histórico em painel; reservar o modal
   para o que exige decisão (conflito de arquivo, confirmação de exclusão).
6. **Tela inicial sem call-to-action.** "No folder open" / "No file open" e
   um ícone de 16 px no canto. Nada indica como começar.
   *Sugestão:* tela de boas-vindas com "Abrir pasta", "Novo projeto",
   projetos recentes e os atalhos principais.
7. **Diagnósticos discretos demais.** O `;` faltando vira um squiggle de
   poucos pixels; sem marcador na régua de scroll, sem painel de problemas,
   sem navegação F8/Shift+F8.
   *Sugestão:* marcas na régua + lista de problemas + atalho de navegação.
8. **Árvore: quatro cliques até o primeiro arquivo, com colapso
   inconsistente.** `src/main` colapsa com `/`, `java` não colapsa com o
   filho único, `com.example` colapsa com `.`.
   *Sugestão:* uma regra só (colapsar toda cadeia de filho único) e
   expandir automaticamente até a primeira pasta com mais de um filho ao
   abrir o projeto.
9. **Quick switcher mostra só o nome do arquivo.** Sem pasta/módulo,
   `Application.java` de dois módulos fica indistinguível; e a seleção
   inicial é o primeiro item, não o "arquivo anterior" que faz Ctrl+E
   alternar entre dois arquivos como nos IDEs.
10. **Sem "Salvar tudo".** `Ctrl+S` salva só a aba ativa; o modal de
    fechamento oferece Salvar/Descartar/Cancelar para uma aba por vez.
    *Sugestão:* `Ctrl+Shift+S`, item no menu File e botão "Salvar todos" no
    modal quando houver mais de uma aba suja.
11. **Sem command palette e atalhos divergentes.** `Ctrl+E` = recentes,
    `Ctrl+Shift+E` = endpoints Spring (no VS Code é o explorador), `F11` =
    zen (convencionalmente tela cheia).
    *Sugestão:* `Ctrl+Shift+P` abrindo uma paleta que liste toda ação de
    menu com seu atalho — resolve descoberta e diverge menos.
12. **Menu de contexto da aba tem só dois itens** (somente leitura,
    histórico). Faltam Fechar / Fechar outras / Fechar à direita / Copiar
    caminho / Revelar na árvore.
13. **Edições não salvas somem sem aviso** se o processo é encerrado por
    fora. O auto-save existe mas é opt-in e desligado por padrão.
    *Sugestão:* rascunho periódico do buffer sujo em `.foxgarden/`,
    restaurado na próxima abertura (o snapshot de histórico já existe, só
    não cobre buffer não salvo).

## Pendências

1. Aplicar as correções de UX acima (nenhuma foi implementada, exceto a do
   `TERM`).
2. Não avaliados/parados de propósito: servidores LSP embutidos via
   `include_bytes!` (~135 MB no binário) — decisão de produto documentada em
   `lsp_manager`, não mexi; e a abertura *inicial* do projeto, que segue
   síncrona (só o refresh virou background).

## Como retomar

```
cargo test --workspace          # 1350 passando
git diff --stat                 # ~40 arquivos, nada commitado
```

Este arquivo é um handoff temporário — apagar quando o trabalho pendente
acabar, movendo o que for permanente para `ISSUES.md`/`TECHNICAL_DEBT.md`
(várias entradas abertas lá agora estão resolvidas: #23 do débito técnico,
#8 e a maior parte da seção de performance do `ISSUES.md`).
