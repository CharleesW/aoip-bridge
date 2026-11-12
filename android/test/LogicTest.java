import org.aoipbridge.listener.*;

/** Testes da logica (sem Android). Rodar: veja build-apk.sh, passo "testes". */
public final class LogicTest {
    static int checks;
    static void check(boolean ok, String what) {
        checks++;
        if (!ok) throw new AssertionError(what);
    }

    static byte[] sap(boolean delete, String sdp) {
        byte[] head = {(byte) (delete ? 0x24 : 0x20), 0, 0x12, 0x34, (byte) 192, (byte) 168, 5, 49};
        byte[] type = "application/sdp\0".getBytes();
        byte[] body = sdp.getBytes();
        byte[] out = new byte[head.length + type.length + body.length];
        System.arraycopy(head, 0, out, 0, 8);
        System.arraycopy(type, 0, out, 8, type.length);
        System.arraycopy(body, 0, out, 8 + type.length, body.length);
        return out;
    }

    public static void main(String[] a) {
        // --- SAP/SDP
        String sdp = "v=0\r\no=- 123 123 IN IP4 192.168.5.49\r\ns=ASIO : 1\r\nc=IN IP4 239.69.5.113/32\r\nt=0 0\r\n"
                + "m=audio 5004 RTP/AVP 97\r\na=rtpmap:97 L24/48000/8\r\na=ptime:1\r\n";
        byte[] p = sap(false, sdp);
        Sap.Flow f = Sap.parse(p, p.length);
        check(f != null && f.name.equals("ASIO : 1") && f.group.equals("239.69.5.113") && f.port == 5004, "sdp basico");
        check(f.pt == 97 && f.bits == 24 && f.rate == 48000 && f.channels == 8 && f.origin.equals("192.168.5.49"), "sdp rtpmap");
        check(f.usable() && !f.delete, "utilizavel");
        p = sap(true, sdp);
        check(Sap.parse(p, p.length).delete, "apagar");
        p = sap(false, sdp.replace("48000", "44100"));
        check(!Sap.parse(p, p.length).usable(), "44100 nao serve");
        p = sap(false, sdp.replace("m=audio", "m=video"));
        check(Sap.parse(p, p.length) == null, "video ignorado");
        check(Sap.parse(new byte[]{1, 2, 3}, 3) == null && Sap.parse(new byte[64], 64) == null, "lixo");

        // --- RTP
        byte[] pkt = new byte[12 + 2 * 3 * 2]; // 2 quadros, 2 canais, L24
        pkt[0] = (byte) 0x80; pkt[1] = 97; pkt[2] = 0x12; pkt[3] = 0x34; pkt[7] = 48;
        pkt[12] = 0x40;                                  // quadro 0, canal 0 = +0,5
        pkt[15] = (byte) 0xC0;                           // quadro 0, canal 1 = -0,5
        pkt[18] = 0x7F; pkt[19] = (byte) 0xFF; pkt[20] = (byte) 0xFF; // quadro 1, canal 0 ~ +1
        Rtp r = new Rtp();
        check(r.parse(pkt, pkt.length) && r.pt == 97 && r.seq == 0x1234 && r.timestamp == 48 && r.length == 12, "rtp cabecalho");
        float[] out = new float[8];
        check(Rtp.decode(pkt, r.offset, r.length, 24, 2, 0, 1, out) == 2, "rtp quadros");
        check(out[0] == 0.5f && out[1] == -0.5f && out[2] > 0.999f && out[3] == 0f, "rtp amostras");
        Rtp.decode(pkt, r.offset, r.length, 24, 2, 1, 5, out);
        check(out[0] == -0.5f && out[1] == 0f, "troca de canal; canal inexistente = silencio");
        check(!r.parse(new byte[]{0, 1, 2}, 3) && !r.parse(new byte[12], 12), "rtp invalido");

        // --- Fila: so toca depois de encher a folga; depois entrega o que entrou
        JitterBuffer jb = new JitterBuffer(480);
        float[] in = new float[96], o = new float[96];
        for (int i = 0; i < 48; i++) { in[i * 2] = 0.25f; in[i * 2 + 1] = -0.25f; }
        jb.push(in, 48);
        jb.pull(o, 48);
        check(o[0] == 0f && jb.fill() == 48, "silencio ate encher a folga");
        for (int i = 0; i < 12; i++) jb.push(in, 48);
        jb.pull(o, 48);
        check(Math.abs(o[10] - 0.25f) < 1e-6 && Math.abs(o[11] + 0.25f) < 1e-6, "entrega o audio");

        // --- Fila: ritmo estavel, com o remetente 300 ppm mais rapido: sem falhas, folga perto do alvo
        jb = new JitterBuffer(960);
        double credit = 0; long pushed = 0;
        float[] tone = new float[96];
        for (int ms = 0; ms < 120_000; ms++) {
            credit += 48 * 1.0003;
            while (credit >= 48) {
                for (int i = 0; i < 48; i++) { float v = (float) Math.sin((pushed + i) * 2 * Math.PI * 440 / 48000); tone[i * 2] = v; tone[i * 2 + 1] = v; }
                jb.push(tone, 48); pushed += 48; credit -= 48;
            }
            jb.pull(o, 48);
        }
        check(jb.underruns == 0 && jb.overruns == 0, "sem falhas com 300 ppm: " + jb.underruns + "/" + jb.overruns);
        check(Math.abs(jb.fill() - 960) < 480, "folga perto do alvo: " + jb.fill());

        // --- Fila: rajadas (Wi-Fi entrega 30 ms de uma vez) cabem na folga de 80 ms
        jb = new JitterBuffer(80 * 48);
        for (int ms = 0; ms < 60_000; ms++) {
            if (ms % 30 == 0) for (int k = 0; k < 30; k++) jb.push(tone, 48);
            jb.pull(o, 48);
        }
        check(jb.underruns == 0 && jb.overruns == 0, "rajadas de 30 ms: " + jb.underruns + "/" + jb.overruns);

        // --- Fila: remetente some -> conta uma falha e volta a tocar quando ele volta
        for (int ms = 0; ms < 500; ms++) jb.pull(o, 48);
        check(jb.underruns == 1 && o[0] == 0f, "esvaziou: " + jb.underruns);
        for (int ms = 0; ms < 1000; ms++) { jb.push(tone, 48); jb.pull(o, 48); }
        check(jb.underruns == 1 && jb.fill() >= 48 * 40, "recuperou");

        // --- Fila: atraso acumulado e cortado de volta para a folga
        jb = new JitterBuffer(480);
        for (int k = 0; k < 200; k++) jb.push(tone, 48);
        jb.pull(o, 48);
        check(jb.fill() <= 480 && jb.overruns >= 1, "corta atraso acumulado: " + jb.fill());

        System.out.println("OK: " + checks + " verificacoes");
    }
}
