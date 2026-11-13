package org.aoipbridge.listener;

/**
 * Cores da identidade visual (docs/design/design-system.md), as mesmas do painel web.
 * Tema escuro e claro; o aplicativo segue o modo do sistema.
 */
final class Palette {
    final boolean dark;
    final int bg, surface, field, border, borderInput, text, text2, text3;
    final int action, onAction, ok, info, warn, err, okBg, infoBg, warnBg, errBg, neutralBg;
    final int route, meterTrack;
    static final int BRAND = 0xFF26A88B; // verde do no central do simbolo, igual nos dois temas

    private Palette(boolean dark) {
        this.dark = dark;
        if (dark) {
            bg = 0xFF171B21; surface = 0xFF1E232B; field = 0xFF262C35; border = 0xFF2E3540; borderInput = 0xFF66727F;
            text = 0xFFE8ECF0; text2 = 0xFFA8B1BC; text3 = 0xFF8A95A3;
            action = 0xFF26A88B; onAction = 0xFF0B1513;
            ok = 0xFF4CC39A; info = 0xFF6AA5E8; warn = 0xFFD9A441; err = 0xFFEC7F7F;
            okBg = 0x29238B68; infoBg = 0x2E3478C8; warnBg = 0x2EB7791F; errBg = 0x29C44747; neutralBg = 0xFF262C35;
            route = 0xFF26A88B; meterTrack = 0xFF0F1216;
        } else {
            bg = 0xFFF4F6F8; surface = 0xFFFFFFFF; field = 0xFFFFFFFF; border = 0xFFD5DBE2; borderInput = 0xFF7F8A97;
            text = 0xFF171B21; text2 = 0xFF4A5564; text3 = 0xFF5F6B79;
            action = 0xFF1B7F69; onAction = 0xFFFFFFFF;
            ok = 0xFF1B7455; info = 0xFF2B66AD; warn = 0xFF8A5A10; err = 0xFFB03A3A;
            okBg = 0xFFE3F3ED; infoBg = 0xFFE4EEFA; warnBg = 0xFFFBF0DC; errBg = 0xFFFBE6E6; neutralBg = 0xFFEEF1F4;
            route = 0xFF1B7F69; meterTrack = 0xFFE3E8ED;
        }
    }

    static Palette of(boolean dark) { return new Palette(dark); }

    /** Cor do texto/icone de um estado: "ok", "bad", "warn", "info" ou "" (neutro). */
    int fg(String kind) {
        switch (kind) {
            case "ok": return ok;
            case "bad": return err;
            case "warn": return warn;
            case "info": return info;
            default: return text2;
        }
    }

    int bgOf(String kind) {
        switch (kind) {
            case "ok": return okBg;
            case "bad": return errBg;
            case "warn": return warnBg;
            case "info": return infoBg;
            default: return neutralBg;
        }
    }
}
