# WIP — code review + usability fixes (2026-09-03/04)

Commitado na branch `ide-henshin`, a partir de `df7944e`.
Suíte: `cargo test --workspace` → **1383 passando, 0 falhando** (antes: 5
falhas por Git LFS). `cargo clippy --workspace --all-targets` → 7 warnings,
todos pré-existentes em código de teste.

**Não rodar `cargo fmt`**: o repositório não é rustfmt-clean por opção
(`HEAD` limpo já acusa 641 diffs). Formatação foi mantida no estilo manual
do entorno.

## Contexto

Partiu de um code review do projeto inteiro (não só do diff). Achados novos
+ itens já listados em `ISSUES.md`/`TECHNICAL_DEBT.md` que continuavam
abertos. Depois veio "corrija tudo" e, em seguida, um pedido de análise de
usabilidade, feita com o app rodando de verdade (ver a seção própria).

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

## Usabilidade (2026-09-04) — corrigida

Levantada com o app real rodando em Xvfb (`:77`), projeto Maven de teste com
9 classes, navegando por cliques/teclado sintéticos (`python-xlib`). Todos os
13 defeitos encontrados foram corrigidos, em quatro commits:

| # | Defeito | Correção |
|---|---------|----------|
| 1 | Abas cortavam na borda; a ativa ficava inalcançável | Barra rolável de uma linha, segue a aba ativa, conta as escondidas |
| 2 | Ícones emoji viravam tofu sem fonte do sistema | `style/icons.rs` com glifos do Nerd Font já embutido; fallback também na família proporcional |
| 3 | Nome longo quebrava em duas linhas na árvore | Truncamento com elipse + tooltip com caminho completo |
| 4 | Barra de status não dizia nada do arquivo | Ln/Col, linguagem, indentação e contagem de erros/avisos |
| 5 | Todo erro era modal bloqueante | `toasts.rs`; modal só para o que exige resposta |
| 6 | Tela inicial sem call-to-action | `panels/welcome.rs`: abrir, criar, projetos recentes, atalhos |
| 7 | Diagnóstico invisível fora da viewport | Régua de marcas à direita + F8/Shift+F8 |
| 8 | Quatro cliques até o primeiro arquivo | Auto-expansão até a primeira pasta que ramifica |
| 9 | Quick switcher sem pasta e sem alternância | Diretório no rótulo; arquivo atual vai para o fim da lista |
| 10 | Sem "Salvar tudo" | File > Save All, Ctrl+Shift+S, e botão no modal de fechamento em lote |
| 11 | Sem command palette; atalhos divergentes | `panels/command_palette.rs` (Ctrl+Shift+P) com os atalhos ao lado |
| 12 | Menu de contexto da aba com dois itens | Fechar / outras / à direita / copiar caminho / mostrar na árvore |
| 13 | Edições não salvas sumiam sem aviso | `core/drafts.rs` + oferta de restauração na abertura |

Extras encontrados durante a verificação:

- Terminal sem `TERM` (o zsh imprimia `tput: No value for $TERM` e o próprio
  prompt como `{nl}{i}{rst}`) — corrigido, `tput colors` responde 256.
- `.foxgarden/` aparecia na árvore do projeto — passou para
  `SKIPPED_DIR_NAMES`.

O colapso de cadeias na árvore (`src/main` com `/`, `com.example` com `.`,
`java` em linha própria) **não** é defeito: é a convenção do IntelliJ,
documentada em `side_panel::collapse_chain`.

## Pendências

1. Não avaliados/parados de propósito: servidores LSP embutidos via
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
