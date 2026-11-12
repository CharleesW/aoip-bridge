package org.aoipbridge.listener;

import android.app.Activity;
import android.content.Intent;
import android.content.SharedPreferences;
import android.graphics.Color;
import android.graphics.Typeface;
import android.media.AudioManager;
import android.os.Build;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.view.Gravity;
import android.view.View;
import android.widget.AdapterView;
import android.widget.ArrayAdapter;
import android.widget.Button;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.Spinner;
import android.widget.TextView;
import java.util.ArrayList;
import java.util.List;

/** Tela unica: escolhe um fluxo da rede e dois canais dele, e toca no celular. */
public final class MainActivity extends Activity {
    private static final int[] LATENCIES_MS = {40, 80, 150, 300};

    private final Handler ui = new Handler(Looper.getMainLooper());
    private SharedPreferences prefs;

    private TextView status, stats;
    private Spinner flowSpin, leftSpin, rightSpin, latencySpin;
    private Button button;
    private List<Sap.Flow> shown = new ArrayList<>();
    private String shownKeys = "";
    private boolean uiPlaying;

    @Override protected void onCreate(Bundle saved) {
        super.onCreate(saved);
        prefs = getPreferences(MODE_PRIVATE);
        setVolumeControlStream(AudioManager.STREAM_MUSIC);
        buildScreen();
        String error = Engine.init(getApplicationContext());
        status.setText(error != null ? error : "Procurando fluxos na rede… (pode levar até 30 s)");
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

    // ------------------------------------------------------------------ tela

    private int dp(int v) { return (int) (v * getResources().getDisplayMetrics().density + 0.5f); }

    private TextView label(LinearLayout parent, String text, int sp, boolean bold) {
        TextView t = new TextView(this);
        t.setText(text);
        t.setTextSize(sp);
        t.setTextColor(Color.WHITE);
        if (bold) t.setTypeface(Typeface.DEFAULT_BOLD);
        t.setPadding(0, dp(12), 0, dp(4));
        parent.addView(t);
        return t;
    }

    private Spinner spinner(LinearLayout parent, List<String> items) {
        Spinner s = new Spinner(this);
        setItems(s, items);
        s.setMinimumHeight(dp(48));
        parent.addView(s, new LinearLayout.LayoutParams(-1, -2));
        return s;
    }

    private void setItems(Spinner s, List<String> items) {
        ArrayAdapter<String> a = new ArrayAdapter<>(this, android.R.layout.simple_spinner_item, items);
        a.setDropDownViewResource(android.R.layout.simple_spinner_dropdown_item);
        s.setAdapter(a);
    }

    private void buildScreen() {
        LinearLayout root = new LinearLayout(this);
        root.setOrientation(LinearLayout.VERTICAL);
        root.setPadding(dp(20), dp(28), dp(20), dp(20));
        root.setBackgroundColor(Color.rgb(18, 20, 26));

        label(root, "AoIP Escuta", 24, true);
        status = label(root, "", 14, false);
        status.setTextColor(Color.rgb(170, 180, 195));

        label(root, "Fluxo da rede", 14, true);
        flowSpin = spinner(root, new ArrayList<String>());
        label(root, "Canal no lado esquerdo", 14, true);
        leftSpin = spinner(root, new ArrayList<String>());
        label(root, "Canal no lado direito", 14, true);
        rightSpin = spinner(root, new ArrayList<String>());

        label(root, "Folga (atraso de segurança)", 14, true);
        List<String> lat = new ArrayList<>();
        for (int ms : LATENCIES_MS) lat.add(ms + " ms" + (ms == 80 ? " (recomendado no Wi-Fi)" : ""));
        latencySpin = spinner(root, lat);
        latencySpin.setSelection(Math.max(0, Math.min(LATENCIES_MS.length - 1, prefs.getInt("latency", 1))));

        button = new Button(this);
        button.setText("Ouvir");
        button.setTextSize(18);
        button.setEnabled(false);
        LinearLayout.LayoutParams bp = new LinearLayout.LayoutParams(-1, dp(60));
        bp.topMargin = dp(20);
        root.addView(button, bp);
        button.setOnClickListener(new View.OnClickListener() {
            @Override public void onClick(View v) { if (Engine.playing == null) start(); else stop(); }
        });

        stats = label(root, "", 14, false);
        stats.setTextColor(Color.rgb(170, 180, 195));
        stats.setGravity(Gravity.START);

        flowSpin.setOnItemSelectedListener(new AdapterView.OnItemSelectedListener() {
            @Override public void onItemSelected(AdapterView<?> p, View v, int pos, long id) { fillChannels(); }
            @Override public void onNothingSelected(AdapterView<?> p) { }
        });

        ScrollView scroll = new ScrollView(this);
        scroll.setBackgroundColor(Color.rgb(18, 20, 26));
        scroll.addView(root);
        setContentView(scroll);
    }

    private Sap.Flow selectedFlow() {
        int i = flowSpin.getSelectedItemPosition();
        return i >= 0 && i < shown.size() ? shown.get(i) : null;
    }

    private void fillChannels() {
        Sap.Flow f = selectedFlow();
        if (f == null) return;
        List<String> ch = new ArrayList<>();
        for (int c = 1; c <= f.channels; c++) ch.add("Canal " + c);
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
            Receiver receiver = Engine.receiver;
            if (receiver == null) return;
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
                button.setEnabled(!now.isEmpty());
                status.setText(now.isEmpty() ? "Procurando fluxos na rede… (pode levar até 30 s)"
                        : now.size() + " fluxo(s) encontrado(s). Escolha e toque em Ouvir.");
            }
            if (playing != null) showStats(receiver, playing);
            if (!receiver.error.isEmpty()) status.setText(receiver.error);
        }
    };

    private void showStats(Receiver receiver, Sap.Flow playing) {
        JitterBuffer buffer = Engine.buffer;
        if (buffer == null) return;
        long age = receiver.lastPacketMs == 0 ? -1 : System.currentTimeMillis() - receiver.lastPacketMs;
        float l = receiver.peakLeft, r = receiver.peakRight;
        receiver.peakLeft = 0; receiver.peakRight = 0;
        String state = age < 0 ? "Esperando o áudio chegar…" : age > 1000 ? "O áudio parou de chegar." : "Recebendo de " + receiver.source;
        status.setText("Ouvindo \"" + playing.name + "\". " + state);
        stats.setText("Nível  E " + bar(l) + "\n       D " + bar(r)
                + "\n\nPacotes recebidos: " + receiver.packets
                + "\nPacotes perdidos: " + receiver.lost
                + "\nFalhas no som: " + buffer.underruns
                + "\nFolga agora: " + (buffer.fill() / 48) + " ms (alvo " + (buffer.target() / 48) + " ms)");
        stats.setTypeface(Typeface.MONOSPACE);
    }

    private static String bar(float peak) {
        int n = peak <= 0.001f ? 0 : (int) Math.max(1, Math.min(20, 20 + 20 * Math.log10(peak) / 3)); // -60 dB .. 0 dB
        StringBuilder b = new StringBuilder();
        for (int i = 0; i < 20; i++) b.append(i < n ? '█' : '·');
        return b.toString();
    }

    // ------------------------------------------------------------------ audio

    private void start() {
        Sap.Flow f = selectedFlow();
        if (f == null || Engine.receiver == null) return;
        int left = leftSpin.getSelectedItemPosition(), right = rightSpin.getSelectedItemPosition();
        int latency = LATENCIES_MS[latencySpin.getSelectedItemPosition()];
        prefs.edit().putString("flow", f.name).putInt("left", left).putInt("right", right)
                .putInt("latency", latencySpin.getSelectedItemPosition()).apply();
        Engine.start(f, left, right, latency);
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
        button.setText(on ? "Parar" : "Ouvir");
        button.setEnabled(on || !shown.isEmpty());
        flowSpin.setEnabled(!on); leftSpin.setEnabled(!on); rightSpin.setEnabled(!on); latencySpin.setEnabled(!on);
        if (!on) {
            shownKeys = ""; // refaz a lista na proxima volta
            stats.setText("");
        }
    }
}
