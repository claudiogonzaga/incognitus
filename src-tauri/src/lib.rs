//! Incognitus — o app avulso. A anonimização e a reidentificação são o plugin `anonimizacao` (plugin/, o mesmo crate que
//! o Graphé Desktop hospeda como módulo) e a janela é a pasta modulo/. Aqui fica só o que é do app: montar o plugin com
//! `init()` (os dados na raiz da pasta de dados do app, como sempre) e a atualização do próprio app (atualizar.rs).
mod atualizar;

use std::path::{Path, PathBuf};
use tauri::Manager;

// ── versão nova do app ─────────────────────────────────────────────────────────────────────────────────────────────

/// A release mais nova (pelo portal, senão pelo GitHub), se for mais nova que esta (SKIA_APP_URL troca o endereço nos testes).
#[tauri::command]
async fn app_checar() -> Result<Option<atualizar::NovaVersao>, String> {
    match std::env::var("SKIA_APP_URL") {
        Ok(url) => atualizar::checar(&url, env!("CARGO_PKG_VERSION")).await,
        Err(_) => atualizar::checar_padrao(env!("CARGO_PKG_VERSION")).await,
    }
}

/// Baixa o instalador da versão nova para a pasta Downloads.
#[tauri::command]
async fn app_baixar<R: tauri::Runtime>(app: tauri::AppHandle<R>, url: String, versao: String) -> Result<String, String> {
    // SKIA_DOWNLOADS (só para testes): baixa numa pasta à parte, não na Downloads do usuário.
    let pasta = match std::env::var_os("SKIA_DOWNLOADS") {
        Some(d) => PathBuf::from(d),
        None => app.path().download_dir().map_err(|e| format!("sem a pasta Downloads: {e}"))?,
    };
    atualizar::baixar(&url, &versao, &pasta).await.map(|p| p.to_string_lossy().to_string())
}

/// Abre o instalador e fecha o app (no Windows o .msi troca a versão; no Mac, arrasta-se o app novo para Aplicativos).
#[tauri::command]
fn app_instalar<R: tauri::Runtime>(app: tauri::AppHandle<R>, caminho: String) -> Result<(), String> {
    atualizar::abrir(Path::new(&caminho))?;
    app.exit(0);
    Ok(())
}

// ── início ─────────────────────────────────────────────────────────────────────────────────────────────────────────

/// O app: as caixas de diálogo, o plugin com `init()` e os comandos da atualização do app. Genérico no runtime para os
/// testes montarem o mesmo app sem abrir janela.
fn montar<R: tauri::Runtime>(b: tauri::Builder<R>) -> tauri::Builder<R> {
    b.plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_anonimizacao::init())
        .invoke_handler(tauri::generate_handler![app_checar, app_baixar, app_instalar])
}

/// O tauri.conf.json, as capabilities e a janela (modulo/), embutidos uma vez só (o app e os testes usam o mesmo).
fn contexto<R: tauri::Runtime>() -> tauri::Context<R> {
    tauri::generate_context!()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Modo de teste com um plano que não abre (JSON em UTF-8): para já, com o motivo, em vez de abrir como app normal.
    if let Some(p) = std::env::var_os("SKIA_E2E") {
        if !tauri_plugin_anonimizacao::plano_e2e_abre() {
            eprintln!("[incognitus] SKIA_E2E: o plano {} não abre como JSON em UTF-8", PathBuf::from(p).display());
            std::process::exit(2);
        }
    }
    montar(tauri::Builder::default()).run(contexto()).expect("erro ao montar o Incognitus");
}

#[cfg(test)]
mod testes {
    //! O app avulso de verdade (tauri.conf.json, capabilities/ e o plugin com `init()`), num runtime de mentira: prova a
    //! ligação da janela `main` com o plugin (as permissões só falham na hora de chamar) sem abrir janela nenhuma.
    use super::*;
    use tauri::test::{get_ipc_response, mock_builder, MockRuntime, INVOKE_KEY};

    static VEZ: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Os testes rodam de `<alvo>/debug/deps`, e no Mac o Tauri procura os recursos em `../Resources`: o atalho aponta
    /// para `<alvo>/debug`, onde o build do app já pôs tessdata/ e lib/. Os sidecars ganham um atalho ao lado do teste
    /// (o plugin os procura ao lado do executável).
    fn preparar_alvo() {
        #[cfg(unix)]
        {
            let deps = std::env::current_exe().unwrap().parent().unwrap().to_path_buf();
            let alvo = deps.parent().unwrap();
            let _ = std::os::unix::fs::symlink(".", alvo.join("Resources"));
            for f in ["pdftotext", "pdftoppm", "tesseract"] {
                let _ = std::os::unix::fs::symlink(alvo.join(f), deps.join(f));
            }
        }
    }

    /// Um app com dados e cache numa pasta à parte (SKIA_DADOS) e a checagem do app num endereço que não responde.
    fn app_com(nome: &str, builder: impl FnOnce() -> tauri::Builder<MockRuntime>) -> (tauri::App<MockRuntime>, PathBuf) {
        let _vez = VEZ.lock().unwrap_or_else(|e| e.into_inner());
        preparar_alvo();
        let dir = std::env::temp_dir().join(format!("incognitus-teste-{nome}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::env::set_var("SKIA_DADOS", &dir);
        std::env::set_var("SKIA_APP_URL", "http://127.0.0.1:1/release.json");
        let app = builder().build(contexto()).expect("app de teste");
        (app, dir)
    }

    /// O app avulso, como o `run()` o monta.
    fn app_de_teste(nome: &str) -> (tauri::App<MockRuntime>, PathBuf) {
        app_com(nome, || montar(mock_builder()))
    }

    fn janela(app: &tauri::App<MockRuntime>, rotulo: &str) -> tauri::WebviewWindow<MockRuntime> {
        tauri::WebviewWindowBuilder::new(app, rotulo, Default::default()).build().expect("janela")
    }

    fn pedir(w: &tauri::WebviewWindow<MockRuntime>, cmd: &str, args: serde_json::Value) -> Result<serde_json::Value, serde_json::Value> {
        get_ipc_response(
            w,
            tauri::webview::InvokeRequest {
                cmd: cmd.into(),
                callback: tauri::ipc::CallbackFn(0),
                error: tauri::ipc::CallbackFn(1),
                url: if cfg!(windows) { "http://tauri.localhost" } else { "tauri://localhost" }.parse().unwrap(),
                body: tauri::ipc::InvokeBody::Json(args),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.to_string(),
            },
        )
        .map(|b| b.deserialize::<serde_json::Value>().unwrap())
    }

    #[test]
    fn a_janela_main_fala_com_o_plugin() {
        let (app, dir) = app_de_teste("ipc");
        let main = janela(&app, "main");
        let nada = serde_json::json!({});
        // quem é o módulo: app avulso, a versão do plugin igual à do app, os dados na RAIZ da pasta de dados (como sempre)
        let info = pedir(&main, "plugin:anonimizacao|info", nada.clone()).expect("info");
        assert_eq!(info["modulo"]["id"], "incognitus");
        assert_eq!(info["modulo"]["versao"], env!("CARGO_PKG_VERSION"));
        assert_eq!(info["hospedado"], false);
        assert_eq!(PathBuf::from(info["dados"].as_str().unwrap()), dir.join("dados"));
        // os Ajustes: gravados e lidos pelo plugin, em ajustes.json na raiz dos dados
        pedir(&main, "plugin:anonimizacao|ajustes_gravar", serde_json::json!({ "ajustes": { "txt": true } })).expect("ajustes_gravar");
        assert!(dir.join("dados").join("ajustes.json").is_file());
        assert_eq!(pedir(&main, "plugin:anonimizacao|ajustes_ler", nada.clone()).expect("ajustes_ler")["txt"], true);
        // o motor em uso: sem nada baixado, o embutido (o front lê o motor.js da pasta da janela)
        let embutido: serde_json::Value = serde_json::from_str(include_str!("../../modulo/motor/manifesto.json")).unwrap();
        let motor = pedir(&main, "plugin:anonimizacao|motor_ativo", nada.clone()).expect("motor_ativo");
        assert_eq!(motor["origem"], "embutido");
        assert_eq!(motor["manifesto"]["sha256"], embutido["sha256"]);
        assert!(motor["js"].is_null());
        // fora do modo de teste de ponta a ponta, não há plano
        if std::env::var_os("SKIA_E2E").is_none() {
            assert!(pedir(&main, "plugin:anonimizacao|e2e_plano", nada.clone()).expect("e2e_plano").is_null());
        }
        // os comandos do app (a atualização) continuam sem prefixo: a checagem chega ao atualizar.rs e falha na rede
        let erro = pedir(&main, "app_checar", nada.clone()).expect_err("app_checar sem servidor");
        assert!(erro.as_str().unwrap_or("").contains("sem conexão"), "{erro}");
        // os nomes antigos, sem o prefixo do plugin, não existem mais
        assert!(pedir(&main, "motor_ativo", nada.clone()).is_err());
        // só a janela `main` recebeu as permissões: outra janela não fala com o plugin
        let outra = janela(&app, "outra");
        let negado = pedir(&outra, "plugin:anonimizacao|info", nada).expect_err("outra janela");
        assert!(negado.as_str().unwrap_or("").contains("not allowed"), "{negado}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// O mesmo plugin como o Graphé Desktop o registra (`init_como_modulo`): diz que está hospedado e guarda os dados e
    /// o cache em `modulos/incognitus/`, dentro das pastas do hospedeiro.
    #[test]
    fn hospedado_guarda_os_dados_em_modulos() {
        let (app, dir) = app_com("hospedado", || mock_builder().plugin(tauri_plugin_dialog::init()).plugin(tauri_plugin_anonimizacao::init_como_modulo()));
        let main = janela(&app, "main");
        let info = pedir(&main, "plugin:anonimizacao|info", serde_json::json!({})).expect("info");
        let dados = dir.join("dados").join("modulos").join("incognitus");
        assert_eq!(info["hospedado"], true);
        assert_eq!(PathBuf::from(info["dados"].as_str().unwrap()), dados);
        pedir(&main, "plugin:anonimizacao|ajustes_gravar", serde_json::json!({ "ajustes": { "txt": true } })).expect("ajustes_gravar");
        assert!(dados.join("ajustes.json").is_file());
        assert!(!dir.join("dados").join("ajustes.json").exists());
        assert!(dir.join("cache").join("modulos").join("incognitus").join("tmp").is_dir());
        assert!(!dir.join("cache").join("tmp").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Os sidecars pela janela, como o app os chama (à mão, depois de `cargo build`, com os PDFs FICTÍCIOS do
    /// testes/gerar_pdfs.py): SKIA_TESTE_PDFS=<pasta> cargo test --manifest-path src-tauri/Cargo.toml -- --ignored sidecars
    #[test]
    #[ignore]
    fn os_sidecars_pela_janela() {
        let pdfs = PathBuf::from(std::env::var_os("SKIA_TESTE_PDFS").expect("SKIA_TESTE_PDFS: a pasta dos PDFs fictícios"));
        let (app, dir) = app_de_teste("sidecars");
        let main = janela(&app, "main");
        let p = |cmd: &str, args: serde_json::Value| pedir(&main, &format!("plugin:anonimizacao|{cmd}"), args).unwrap_or_else(|e| panic!("{cmd}: {e}"));
        let f = p("info_app", serde_json::json!({}));
        let versoes = f["ferramentas"]["Ok"].as_array().unwrap_or_else(|| panic!("as ferramentas não rodaram: {f}")).clone();
        assert_eq!(versoes.len(), 3);
        println!("ferramentas: {versoes:?}");
        // texto.pdf: o pdftotext -layout, com um \f no fim de cada uma das 3 páginas
        let a = p("preparar_arquivo", serde_json::json!({ "caminho": pdfs.join("texto.pdf") }));
        let id = a["id"].as_str().unwrap().to_string();
        assert!(dir.join("cache").join("tmp").join(&id).join("entrada.pdf").is_file());
        let texto = p("extrair_texto", serde_json::json!({ "id": id }));
        assert_eq!(texto.as_str().unwrap().matches('\u{c}').count(), 3);
        p("limpar_arquivo", serde_json::json!({ "id": id }));
        // escaneado.pdf: o OCR da página 1 (pdftoppm + tesseract em português)
        let b = p("preparar_arquivo", serde_json::json!({ "caminho": pdfs.join("escaneado.pdf") }));
        let id = b["id"].as_str().unwrap().to_string();
        let ocr = p("ocr_pagina", serde_json::json!({ "id": id, "pagina": 1 }));
        let letras = ocr.as_str().unwrap().chars().filter(|c| c.is_alphabetic()).count();
        println!("OCR da página 1: {letras} letras");
        assert!(letras >= 50 && ocr.as_str().unwrap().contains("BOLETIM"));
        p("limpar_arquivo", serde_json::json!({ "id": id }));
        assert_eq!(std::fs::read_dir(dir.join("cache").join("tmp")).unwrap().count(), 0, "sobrou arquivo temporário");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
