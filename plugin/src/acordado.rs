//! Enquanto a fila roda, o app declara ao sistema uma atividade iniciada pelo usuário (no macOS, `NSProcessInfo
//! beginActivityWithOptions:reason:`). Sem isso, com a janela fora da tela (outra mesa, app em tela cheia, minimizada),
//! o macOS suspende o processo da página — o motor e a leitura rigorosa congelam no meio, sem CPU, até a janela voltar
//! (visto em 14/11/2026). A atividade também impede o repouso automático do Mac durante um lote longo; termina com a fila.

#[cfg(target_os = "macos")]
mod mac {
    use objc2::rc::Retained;
    use objc2::runtime::{NSObjectProtocol, ProtocolObject};
    use objc2_foundation::{NSActivityOptions, NSProcessInfo, NSString};
    use std::sync::Mutex;

    struct Atividade(Retained<ProtocolObject<dyn NSObjectProtocol>>);
    // o objeto só é guardado e devolvido ao NSProcessInfo, que aceita a chamada de qualquer thread
    unsafe impl Send for Atividade {}

    static ATUAL: Mutex<Option<Atividade>> = Mutex::new(None);

    pub fn ligar(motivo: &str) -> bool {
        let mut g = ATUAL.lock().unwrap_or_else(|e| e.into_inner());
        if g.is_none() {
            let a = NSProcessInfo::processInfo().beginActivityWithOptions_reason(NSActivityOptions::UserInitiated, &NSString::from_str(motivo));
            *g = Some(Atividade(a));
        }
        true
    }

    pub fn desligar() -> bool {
        if let Some(a) = ATUAL.lock().unwrap_or_else(|e| e.into_inner()).take() {
            unsafe { NSProcessInfo::processInfo().endActivity(&a.0) };
        }
        false
    }
}

/// Cada WebView do app (a janela do Incognitus ou a do hospedeiro): o WebKit põe para dormir a página cuja janela saiu da
/// tela ("process suppression" pela visibilidade) e freia os timers dela; com isso o lote congelava sem CPU com a janela
/// em outra mesa (14/11/2026: nem a atividade do NSProcessInfo nem o backgroundThrottling bastaram). As duas preferências
/// do WKPreferences são privadas, mas estáveis há anos; só são chamadas se o WebKit as tiver.
#[cfg(target_os = "macos")]
pub fn webview_sem_sono(wk: *mut std::ffi::c_void) {
    use objc2::runtime::{AnyObject, Bool, Sel};
    use objc2::{msg_send, sel};
    if wk.is_null() {
        return;
    }
    unsafe {
        let wk = wk as *mut AnyObject;
        let config: *mut AnyObject = msg_send![wk, configuration];
        if config.is_null() {
            return;
        }
        let prefs: *mut AnyObject = msg_send![config, preferences];
        if prefs.is_null() {
            return;
        }
        let tem = |s: Sel| -> bool { let r: Bool = msg_send![prefs, respondsToSelector: s]; r.as_bool() };
        if tem(sel!(_setPageVisibilityBasedProcessSuppressionEnabled:)) {
            let _: () = msg_send![prefs, _setPageVisibilityBasedProcessSuppressionEnabled: Bool::NO];
        }
        if tem(sel!(_setHiddenPageDOMTimerThrottlingEnabled:)) {
            let _: () = msg_send![prefs, _setHiddenPageDOMTimerThrottlingEnabled: Bool::NO];
        }
        // a política de agendamento da página inativa (pública, macOS 14+): nenhuma (2), sem suspender nem frear — a mesma
        // do "backgroundThrottling": "disabled", posta aqui também para valer no hospedeiro
        let mut feitos = Vec::new();
        if tem(sel!(setInactiveSchedulingPolicy:)) {
            let _: () = msg_send![prefs, setInactiveSchedulingPolicy: 2isize];
            feitos.push("inativa=nenhuma");
        }
        // a janela em outra mesa conta como escondida pela detecção de oclusão: sem ela, o WebKit olha só se a janela está
        // aberta (depois de uns 5 minutos fora da tela a página era suspensa de novo, 14/11/2026)
        let r: Bool = msg_send![wk, respondsToSelector: sel!(_setWindowOcclusionDetectionEnabled:)];
        if r.as_bool() {
            let _: () = msg_send![wk, _setWindowOcclusionDetectionEnabled: Bool::NO];
            feitos.push("oclusão=não");
        }
        if std::env::var_os("SKIA_E2E").is_some() {
            eprintln!("[incognitus] webview sem sono: {}", feitos.join(", "));
        }
    }
}

/// Liga ou desliga a atividade; devolve se ficou ligada (fora do macOS, nada a fazer: false).
pub fn acordado(ligar: bool) -> bool {
    #[cfg(target_os = "macos")]
    {
        if ligar { mac::ligar("Incognitus: anonimizando a fila") } else { mac::desligar() }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = ligar;
        false
    }
}

#[cfg(test)]
mod testes {
    #[test]
    fn liga_e_desliga() {
        let ligado = super::acordado(true);
        assert_eq!(ligado, cfg!(target_os = "macos"));
        assert!(super::acordado(true) == ligado, "ligar duas vezes não cria outra atividade");
        assert!(!super::acordado(false));
        assert!(!super::acordado(false), "desligar sem atividade não falha");
    }
}
