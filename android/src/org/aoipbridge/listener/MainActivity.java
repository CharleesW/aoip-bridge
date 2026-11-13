package org.aoipbridge.listener;

import android.app.Activity;
import android.content.Intent;
import android.content.SharedPreferences;
import android.content.res.ColorStateList;
import android.content.res.Configuration;
import android.graphics.Typeface;
import android.graphics.drawable.Drawable;
import android.graphics.drawable.GradientDrawable;
import android.graphics.drawable.RippleDrawable;
import android.media.AudioManager;
import android.os.Build;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.text.TextUtils;
import android.view.Gravity;
import android.view.View;
import android.view.ViewGroup;
import android.widget.AdapterView;
import android.widget.ArrayAdapter;
import android.widget.Button;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.Spinner;
import android.widget.TextView;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;

/** Tela unica: escolhe um fluxo da rede e dois canais dele, e toca no celular. */
public final class MainActivity extends Activity {
    /** Folgas oferecidas; 0 = automatica. */
    private static final int[] FOLGA_MS = {0, 10, 20, 40, 80, 150, 300};
    private static final String[] FOLGA_NOME = {
        "Automática (recomendado)", "10 ms · baixa latência", "20 ms · baixa latência",
        "40 ms", "80 ms", "150 ms", "300 ms"};
    private static final String[] FOLGA_DICA = {
        "Começa baixa (10 ms no cabo, 20 ms no Wi-Fi) e aumenta sozinha cada vez que o som falhar, até "
            + Engine.AUTO_MAX_MS + " ms.",
        "Para cabo de rede, ou Wi-Fi 5 GHz excelente perto do roteador.",
        "Para Wi-Fi bom: 5 GHz, sinal forte e pouca gente usando.",
        "Para Wi-Fi comum.",
        "Para Wi-Fi com muita gente usando.",
        "Para Wi-Fi fraco ou longe do roteador.",
        "Para a pior rede: o som chega bem atrasado, mas quase nunca falha."};

    private final Handler ui = new Handler(Looper.getMainLooper());
    private SharedPreferences prefs;
    private Palette c;

    private LinearLayout badge, alert, recCard;
    private Widgets.StatusIcon badgeIcon, alertIcon;
    private TextView badgeText, alertText, status, folgaHint, netText;
    private Spinner flowSpin, leftSpin, rightSpin, folgaSpin;
    private final List<View> fields = new ArrayList<>();
    private Button button;
    private Widgets.Meter meterL, meterR, lastMeter;
    private TextView dbL, dbR, vSource, vPackets, vLost, vFails, vFolga, vOut, vTotal;
    private List<Sap.Flow> shown = new ArrayList<>();
    private String shownKeys = "", initError;
    private boolean uiPlaying;
    private int ticks;

    @Override protected void onCreate(Bundle saved) {
        super.onCreate(saved);
        prefs = getPreferences(MODE_PRIVATE);
        boolean night = (getResources().getConfiguration().uiMode & Configuration.UI_MODE_NIGHT_MASK) == Configuration.UI_MODE_NIGHT_YES;
        c = Palette.of(night);
        setVolumeControlStream(AudioManager.STREAM_MUSIC);
        systemBars();
        buildScreen();
        initError = Engine.init(getApplicationContext());
        if (Build.VERSION.SDK_INT >= 33 && checkSelfPermission("android.permission.POST_NOTIFICATIONS") != 0) {
            requestPermissions(new String[]{"android.permission.POST_NOTIFICATIONS"}, 1); // aviso "Tocando" com botao Parar
        }
        ui.post(tick);
    }

    @Override protected void onDestroy() {
        ui.removeCallbacks(tick);
        if (Engine.playing == null) Engine.shutdown(); // tocando: continua com a tela fechada
        super.onDestroy();
    }

    // ------------------------------------------------------------------ pecas da tela

    private int dp(float v) { return (int) (v * getResources().getDisplayMetrics().density + 0.5f); }

    /** Versao instalada ("0.3.2"), para aparecer em qualquer captura de tela. */
    private String version() {
        try {
            return getPackageManager().getPackageInfo(getPackageName(), 0).versionName;
        } catch (Exception e) {
            return "";
        }
    }

    private void systemBars() {
        getWindow().setStatusBarColor(c.bg);
        getWindow().setNavigationBarColor(c.bg);
        if (!c.dark) {
            int flags = View.SYSTEM_UI_FLAG_LIGHT_STATUS_BAR;
            if (Build.VERSION.SDK_INT >= 26) flags |= View.SYSTEM_UI_FLAG_LIGHT_NAVIGATION_BAR;
            getWindow().getDecorView().setSystemUiVisibility(flags);
        }
    }

    private GradientDrawable box(int fill, int stroke, float radiusDp) {
        GradientDrawable g = new GradientDrawable();
        g.setColor(fill);
        g.setCornerRadius(dp(radiusDp));
        if (stroke != 0) g.setStroke(dp(1), stroke);
        return g;
    }

    private TextView text(ViewGroup parent, String s, float sp, int color, boolean bold) {
        TextView t = new TextView(this);
        t.setText(s);
        t.setTextSize(sp);
        t.setTextColor(color);
        t.setFontFeatureSettings("tnum");
        if (bold) t.setTypeface(Typeface.create("sans-serif-medium", Typeface.NORMAL));
        parent.addView(t);
        return t;
    }

    private static LinearLayout.LayoutParams lp(int w, int h, int top) {
        LinearLayout.LayoutParams p = new LinearLayout.LayoutParams(w, h);
        p.topMargin = top;
        return p;
    }

    /** Bloco com titulo (superficie, borda fina, sem sombra). */
    private LinearLayout card(LinearLayout root, String title) {
        LinearLayout k = new LinearLayout(this);
        k.setOrientation(LinearLayout.VERTICAL);
        k.setBackground(box(c.surface, c.border, 8));
        k.setPadding(dp(16), dp(14), dp(16), dp(16));
        root.addView(k, lp(-1, -2, dp(12)));
        TextView t = text(k, title, 16, c.text, true);
        if (Build.VERSION.SDK_INT >= 28) t.setAccessibilityHeading(true);
        return k;
    }

    /** Lista de escolha com rotulo, num campo arredondado. */
    private Spinner choice(LinearLayout parent, String label) {
        TextView l = text(parent, label, 13, c.text2, false);
        l.setPadding(0, dp(12), 0, dp(6));
        LinearLayout f = new LinearLayout(this);
        f.setBackground(box(c.field, c.borderInput, 6));
        f.setMinimumHeight(dp(48));
        f.setGravity(Gravity.CENTER_VERTICAL);
        Spinner s = new Spinner(this, Spinner.MODE_DROPDOWN);
        s.setBackgroundTintList(ColorStateList.valueOf(c.text2));
        s.setPopupBackgroundDrawable(box(c.surface, c.border, 8));
        s.setContentDescription(label);
        setItems(s, new ArrayList<String>());
        f.addView(s, new LinearLayout.LayoutParams(-1, -2));
        parent.addView(f, new LinearLayout.LayoutParams(-1, -2));
        fields.add(f);
        return s;
    }

    private void setItems(Spinner s, List<String> items) {
        ArrayAdapter<String> a = new ArrayAdapter<String>(this, android.R.layout.simple_spinner_item, items) {
            @Override public View getView(int pos, View v, ViewGroup p) { return style((TextView) super.getView(pos, v, p), false); }
            @Override public View getDropDownView(int pos, View v, ViewGroup p) { return style((TextView) super.getDropDownView(pos, v, p), true); }
        };
        a.setDropDownViewResource(android.R.layout.simple_spinner_dropdown_item);
        s.setAdapter(a);
    }

    private TextView style(TextView t, boolean drop) {
        t.setTextColor(c.text);
        t.setTextSize(16);
        t.setSingleLine(true);
        t.setEllipsize(TextUtils.TruncateAt.END);
        t.setPadding(dp(12), dp(drop ? 12 : 4), dp(12), dp(drop ? 12 : 4));
        t.setMinHeight(dp(48));
        t.setGravity(Gravity.CENTER_VERTICAL | Gravity.START);
        return t;
    }

    /** Linha "nome ........ valor" do bloco Recepcao. */
    private TextView row(LinearLayout parent, String key) {
        LinearLayout r = new LinearLayout(this);
        r.setPadding(0, dp(6), 0, dp(6));
        TextView k = text(r, key, 14, c.text2, false);
        k.setLayoutParams(new LinearLayout.LayoutParams(0, -2, 1));
        TextView v = text(r, "—", 14, c.text, false);
        v.setGravity(Gravity.END);
        parent.addView(r);
        return v;
    }

    private void buildScreen() {
        LinearLayout root = new LinearLayout(this);
        root.setOrientation(LinearLayout.VERTICAL);
        root.setPadding(dp(16), dp(20), dp(16), dp(24));

        // Cabecalho: simbolo, nome com a versao, e estado
        LinearLayout head = new LinearLayout(this);
        head.setGravity(Gravity.CENTER_VERTICAL);
        head.addView(new Widgets.BrandMark(this, c.text, c.bg), new LinearLayout.LayoutParams(dp(36), dp(36)));
        LinearLayout names = new LinearLayout(this);
        names.setOrientation(LinearLayout.VERTICAL);
        names.setPadding(dp(10), 0, dp(8), 0);
        LinearLayout titleRow = new LinearLayout(this);
        titleRow.setGravity(Gravity.BOTTOM);
        TextView title = text(titleRow, "AoIP Escuta", 20, c.text, true);
        if (Build.VERSION.SDK_INT >= 28) title.setAccessibilityHeading(true);
        String ver = version();
        if (!ver.isEmpty()) {
            TextView v = text(titleRow, "v" + ver, 13, c.text3, false);
            v.setPadding(dp(8), 0, 0, dp(2));
            v.setContentDescription("versão " + ver);
        }
        names.addView(titleRow);
        text(names, "AOIP-Bridge", 12, c.text3, false);
        head.addView(names, new LinearLayout.LayoutParams(0, -2, 1));
        badge = new LinearLayout(this);
        badge.setGravity(Gravity.CENTER_VERTICAL);
        badge.setPadding(dp(10), dp(5), dp(12), dp(5));
        badgeIcon = new Widgets.StatusIcon(this);
        badge.addView(badgeIcon, new LinearLayout.LayoutParams(dp(14), dp(14)));
        badgeText = text(badge, "", 13, c.text2, true);
        badgeText.setPadding(dp(6), 0, 0, 0);
        head.addView(badge);
        root.addView(head);

        status = text(root, "", 14, c.text2, false);
        status.setPadding(0, dp(12), 0, 0);
        status.setAccessibilityLiveRegion(View.ACCESSIBILITY_LIVE_REGION_POLITE);

        // Aviso (erro ou atencao): icone + texto, nunca so cor
        alert = new LinearLayout(this);
        alert.setPadding(dp(12), dp(10), dp(12), dp(10));
        alertIcon = new Widgets.StatusIcon(this);
        LinearLayout.LayoutParams ip = new LinearLayout.LayoutParams(dp(16), dp(16));
        ip.topMargin = dp(2);
        alert.addView(alertIcon, ip);
        alertText = text(alert, "", 14, c.text, false);
        alertText.setPadding(dp(8), 0, 0, 0);
        alert.setVisibility(View.GONE);
        alert.setAccessibilityLiveRegion(View.ACCESSIBILITY_LIVE_REGION_POLITE);
        root.addView(alert, lp(-1, -2, dp(12)));

        // O que ouvir
        LinearLayout src = card(root, "O que ouvir");
        flowSpin = choice(src, "Fluxo da rede");
        LinearLayout lr = new LinearLayout(this);
        LinearLayout colL = new LinearLayout(this), colR = new LinearLayout(this);
        colL.setOrientation(LinearLayout.VERTICAL);
        colR.setOrientation(LinearLayout.VERTICAL);
        lr.addView(colL, new LinearLayout.LayoutParams(0, -2, 1));
        LinearLayout.LayoutParams half2 = new LinearLayout.LayoutParams(0, -2, 1);
        half2.leftMargin = dp(12);
        lr.addView(colR, half2);
        src.addView(lr);
        leftSpin = choice(colL, "Lado esquerdo");
        rightSpin = choice(colR, "Lado direito");

        // Atraso
        LinearLayout lat = card(root, "Atraso");
        folgaSpin = choice(lat, "Folga de segurança");
        setItems(folgaSpin, Arrays.asList(FOLGA_NOME));
        int saved = prefs.getInt("folga_ms", 0), pick = 0;
        for (int i = 0; i < FOLGA_MS.length; i++) if (FOLGA_MS[i] == saved) pick = i;
        folgaSpin.setSelection(pick);
        folgaHint = text(lat, "", 13, c.text3, false);
        folgaHint.setPadding(0, dp(8), 0, 0);
        netText = text(lat, "", 13, c.text2, false);
        netText.setPadding(0, dp(10), 0, 0);

        // Botao principal
        button = new Button(this);
        button.setAllCaps(false);
        button.setTextSize(18);
        button.setTypeface(Typeface.create("sans-serif-medium", Typeface.NORMAL));
        button.setStateListAnimator(null);
        button.setElevation(0);
        root.addView(button, lp(-1, dp(56), dp(16)));
        button.setOnClickListener(new View.OnClickListener() {
            @Override public void onClick(View v) { if (Engine.playing == null) start(); else stop(); }
        });

        // Recepcao (so tocando)
        recCard = card(root, "Recepção");
        dbL = meter(recCard, "E");
        meterL = lastMeter;
        dbR = meter(recCard, "D");
        meterR = lastMeter;
        vSource = row(recCard, "Recebendo de");
        vPackets = row(recCard, "Pacotes recebidos");
        vLost = row(recCard, "Pacotes perdidos");
        vFails = row(recCard, "Falhas no som");
        vFolga = row(recCard, "Folga agora");
        vOut = row(recCard, "Saída do celular");
        vTotal = row(recCard, "Atraso no celular");
        TextView note = text(recCard, "O atraso no celular é aproximado: soma a folga e a saída de som, sem contar o atraso interno do aparelho e o de quem envia.", 12, c.text3, false);
        note.setPadding(0, dp(8), 0, 0);
        recCard.setVisibility(View.GONE);

        flowSpin.setOnItemSelectedListener(new AdapterView.OnItemSelectedListener() {
            @Override public void onItemSelected(AdapterView<?> p, View v, int pos, long id) { fillChannels(); }
            @Override public void onNothingSelected(AdapterView<?> p) { }
        });
        folgaSpin.setOnItemSelectedListener(new AdapterView.OnItemSelectedListener() {
            @Override public void onItemSelected(AdapterView<?> p, View v, int pos, long id) { folgaHint.setText(FOLGA_DICA[pos]); }
            @Override public void onNothingSelected(AdapterView<?> p) { }
        });
        folgaHint.setText(FOLGA_DICA[pick]);

        ScrollView scroll = new ScrollView(this);
        scroll.setBackgroundColor(c.bg);
        scroll.addView(root);
        setContentView(scroll);
        setBadge("info", "Procurando");
        paintButton();
    }

    private TextView meter(LinearLayout parent, String side) {
        LinearLayout r = new LinearLayout(this);
        r.setGravity(Gravity.CENTER_VERTICAL);
        r.setPadding(0, dp(8), 0, dp(4));
        TextView s = text(r, side, 13, c.text2, true);
        s.setMinWidth(dp(20));
        lastMeter = new Widgets.Meter(this, c);
        r.addView(lastMeter, new LinearLayout.LayoutParams(0, dp(10), 1));
        TextView db = text(r, "—", 13, c.text2, false);
        db.setMinWidth(dp(64));
        db.setGravity(Gravity.END);
        db.setTypeface(Typeface.MONOSPACE);
        parent.addView(r);
        return db;
    }

    private void setBadge(String kind, String label) {
        badge.setBackground(box(c.bgOf(kind), 0, 999));
        badgeIcon.set(kind, c.fg(kind));
        badgeText.setTextColor(c.fg(kind));
        badgeText.setText(label);
        badge.setContentDescription("Estado: " + label);
    }

    private void showAlert(String kind, String msg) {
        if (msg == null || msg.isEmpty()) {
            alert.setVisibility(View.GONE);
            return;
        }
        alert.setBackground(box(c.bgOf(kind), 0, 8));
        alertIcon.set(kind, c.fg(kind));
        if (!msg.contentEquals(alertText.getText())) alertText.setText(msg);
        alert.setVisibility(View.VISIBLE);
    }

    /** Botao: verde da marca para Ouvir; neutro com borda para Parar; apagado quando nao da. */
    private void paintButton() {
        boolean on = uiPlaying, enabled = on || !shown.isEmpty();
        button.setText(on ? "Parar" : "Ouvir");
        button.setEnabled(enabled);
        int fill = on ? c.field : enabled ? c.action : c.neutralBg;
        int stroke = on ? c.borderInput : 0;
        Drawable content = box(fill, stroke, 8);
        int rippleColor = on ? (c.dark ? 0x33FFFFFF : 0x22000000) : 0x33FFFFFF;
        button.setBackground(new RippleDrawable(ColorStateList.valueOf(rippleColor), content, box(0xFF000000, 0, 8)));
        button.setTextColor(on ? c.text : enabled ? c.onAction : c.text3);
        for (View f : fields) f.setAlpha(on ? 0.55f : 1f);
        flowSpin.setEnabled(!on); leftSpin.setEnabled(!on); rightSpin.setEnabled(!on); folgaSpin.setEnabled(!on);
    }

    // ------------------------------------------------------------------ dados

    private Sap.Flow selectedFlow() {
        int i = flowSpin.getSelectedItemPosition();
        return i >= 0 && i < shown.size() ? shown.get(i) : null;
    }

    private void fillChannels() {
        Sap.Flow f = selectedFlow();
        if (f == null) return;
        List<String> ch = new ArrayList<>();
        for (int k = 1; k <= f.channels; k++) ch.add("Canal " + k);
        boolean same = f.name.equals(prefs.getString("flow", ""));
        int l = same ? prefs.getInt("left", 0) : 0, r = same ? prefs.getInt("right", 1) : 1;
        setItems(leftSpin, ch);
        setItems(rightSpin, ch);
        leftSpin.setSelection(Math.min(l, f.channels - 1));
        rightSpin.setSelection(Math.min(r, f.channels - 1));
    }

    /** Atualiza a lista de fluxos e os numeros duas vezes por segundo. */
    private final Runnable tick = new Runnable() {
        @Override public void run() {
            ui.postDelayed(this, 500);
            ticks++;
            Receiver receiver = Engine.receiver;
            if (receiver == null) {
                // Sem rede: tenta de novo a cada 3 s (o cabo ou o Wi-Fi pode ter acabado de ligar).
                if (ticks % 6 == 0) initError = Engine.init(getApplicationContext());
                setBadge("bad", "Sem rede");
                status.setText("");
                showAlert("bad", initError != null ? initError : "Sem rede.");
                netText.setText("");
                return;
            }
            netText.setText("Rede: " + Engine.netLabel + (Engine.cable ? " (bom para baixa latência)" : ""));
            Sap.Flow playing = Engine.playing;
            if (uiPlaying != (playing != null)) showPlaying(playing != null);
            List<Sap.Flow> now = receiver.flows();
            StringBuilder keys = new StringBuilder();
            for (Sap.Flow f : now) keys.append(f.key()).append('/').append(f.channels).append(';');
            if (playing == null && !keys.toString().equals(shownKeys)) {
                Sap.Flow before = selectedFlow();
                String want = before != null ? before.key() : null;
                String saved = prefs.getString("flow", "");
                shown = now;
                shownKeys = keys.toString();
                List<String> names = new ArrayList<>();
                int pick = 0;
                for (int i = 0; i < now.size(); i++) {
                    names.add(now.get(i).toString());
                    if (want != null ? now.get(i).key().equals(want) : now.get(i).name.equals(saved)) pick = i;
                }
                setItems(flowSpin, names);
                if (!now.isEmpty()) flowSpin.setSelection(pick);
                paintButton();
            }
            if (playing == null) {
                setBadge(now.isEmpty() ? "info" : "ok", now.isEmpty() ? "Procurando" : "Pronto");
                status.setText(now.isEmpty() ? "Procurando fluxos na rede… (pode levar até 30 s)"
                        : now.size() + " fluxo(s) encontrado(s). Escolha e toque em Ouvir.");
                showAlert("bad", receiver.error);
            } else {
                showStats(receiver, playing);
            }
        }
    };

    private void showStats(Receiver receiver, Sap.Flow playing) {
        JitterBuffer buffer = Engine.buffer;
        if (buffer == null) return;
        long age = receiver.lastPacketMs == 0 ? -1 : System.currentTimeMillis() - receiver.lastPacketMs;
        float l = receiver.peakLeft, r = receiver.peakRight;
        receiver.peakLeft = 0; receiver.peakRight = 0;
        if (age < 0) setBadge("info", "Esperando");
        else if (age > 1000) setBadge("warn", "Sem áudio");
        else setBadge("ok", "Tocando");
        status.setText(age < 0 ? "Ouvindo \"" + playing.name + "\". Esperando o áudio chegar…"
                : age > 1000 ? "Ouvindo \"" + playing.name + "\". O áudio parou de chegar."
                : "Ouvindo \"" + playing.name + "\".");

        meterL.setPeak(l); meterR.setPeak(r);
        dbL.setText(dbText(meterL.db()));
        dbR.setText(dbText(meterR.db()));
        meterL.setContentDescription("Nível do lado esquerdo: " + dbText(meterL.db()));
        meterR.setContentDescription("Nível do lado direito: " + dbText(meterR.db()));

        int folga = buffer.target() / 48, out = Engine.outputMs;
        vSource.setText(receiver.source.isEmpty() ? "—" : receiver.source);
        vPackets.setText(String.valueOf(receiver.packets));
        vLost.setText(String.valueOf(receiver.lost));
        vFails.setText(String.valueOf(buffer.underruns));
        vFails.setTextColor(buffer.underruns > 0 ? c.warn : c.text);
        vFolga.setText((buffer.fill() / 48) + " ms (alvo " + folga + " ms" + (buffer.automatic() ? ", automática" : "") + ")");
        int raised = Engine.outputRaises;
        vOut.setText(out <= 0 ? "—" : out + " ms" + (raised > 0 ? " (subiu " + raised + "x)" : ""));
        vTotal.setText("≈ " + (folga + out) + " ms");

        if (!receiver.error.isEmpty()) {
            showAlert("bad", receiver.error);
        } else if (buffer.automatic() && buffer.raises > 0) {
            showAlert("info", "O som falhou " + buffer.underruns + " vez(es); a folga subiu sozinha para " + folga + " ms."
                    + (folga >= Engine.AUTO_MAX_MS ? " Chegou ao máximo: a rede está ruim para áudio." : ""));
        } else if (!buffer.automatic() && buffer.underruns > 0) {
            showAlert("warn", "O som falhou " + buffer.underruns + " vez(es) com folga fixa de " + folga
                    + " ms. Pare e escolha uma folga maior ou Automática.");
        } else {
            showAlert("", null);
        }
    }

    private static String dbText(float db) {
        return db <= -60 ? "—" : String.format(java.util.Locale.ROOT, "%.0f dB", db).replace('-', '−');
    }

    // ------------------------------------------------------------------ audio

    private void start() {
        Sap.Flow f = selectedFlow();
        if (f == null || Engine.receiver == null) return;
        int left = leftSpin.getSelectedItemPosition(), right = rightSpin.getSelectedItemPosition();
        int folga = FOLGA_MS[Math.max(0, folgaSpin.getSelectedItemPosition())];
        prefs.edit().putString("flow", f.name).putInt("left", left).putInt("right", right).putInt("folga_ms", folga).apply();
        Engine.start(f, left, right, folga);
        Intent service = new Intent(this, PlayService.class);
        if (Build.VERSION.SDK_INT >= 26) startForegroundService(service); else startService(service);
        showPlaying(true);
    }

    private void stop() {
        Engine.stop();
        stopService(new Intent(this, PlayService.class));
        showPlaying(false);
    }

    /** Ajusta a tela para "tocando" ou "parado" (tambem quando o Parar veio do aviso). */
    private void showPlaying(boolean on) {
        uiPlaying = on;
        recCard.setVisibility(on ? View.VISIBLE : View.GONE);
        if (!on) {
            shownKeys = ""; // refaz a lista na proxima volta
            showAlert("", null);
        }
        paintButton();
    }
}
