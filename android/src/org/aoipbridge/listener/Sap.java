package org.aoipbridge.listener;

/** Anuncios SAP (239.255.255.255:9875) com a descricao SDP de um fluxo AES67. */
public final class Sap {
    public static final String GROUP = "239.255.255.255";
    public static final int PORT = 9875;

    public static final class Flow {
        public String name = "", group = "", origin = "";
        public int port, pt = 97, channels = 1, rate, bits;
        public boolean delete;
        public long seenMs;

        public boolean usable() {
            return rate == 48000 && (bits == 24 || bits == 16) && channels >= 1 && port > 0 && !group.isEmpty();
        }
        public String key() { return origin + "|" + name; }
        @Override public String toString() { return name + " (" + channels + " canais)"; }
    }

    /** Le um pacote SAP. Devolve null se nao for um anuncio de audio que entendemos. */
    public static Flow parse(byte[] b, int len) {
        if (len < 8 || (b[0] & 0xE0) != 0x20) return null;      // versao 1
        if ((b[0] & 0x10) != 0 || (b[0] & 0x03) != 0) return null; // IPv6, cifrado ou comprimido
        Flow f = new Flow();
        f.delete = (b[0] & 0x04) != 0;
        int pos = 8 + (b[1] & 0xFF) * 4;
        if (pos >= len) return null;
        String text = new String(b, pos, len - pos, java.nio.charset.StandardCharsets.ISO_8859_1);
        int start = text.indexOf("v=0");
        if (start < 0) return null;
        boolean media = false;
        for (String raw : text.substring(start).split("\n")) {
            String line = raw.trim();
            if (line.length() < 2 || line.charAt(1) != '=') continue;
            String v = line.substring(2).trim();
            switch (line.charAt(0)) {
                case 's': f.name = v; break;
                case 'o': { String[] p = v.split("\\s+"); if (p.length >= 6) f.origin = p[5]; break; }
                case 'c': { String[] p = v.split("\\s+"); if (p.length >= 3) f.group = p[2].split("/")[0]; break; }
                case 'm': {
                    if (media) return f.rate == 0 ? null : f; // so a primeira midia
                    String[] p = v.split("\\s+");
                    if (p.length < 4 || !p[0].equals("audio")) return null;
                    f.port = num(p[1].split("/")[0]);
                    f.pt = num(p[3]);
                    media = true;
                    break;
                }
                case 'a':
                    if (media && v.startsWith("rtpmap:")) {
                        String[] p = v.substring(7).split("\\s+");
                        if (p.length >= 2 && num(p[0]) == f.pt) {
                            String[] e = p[1].split("/");
                            f.bits = e[0].equalsIgnoreCase("L24") ? 24 : e[0].equalsIgnoreCase("L16") ? 16 : 0;
                            f.rate = e.length > 1 ? num(e[1]) : 0;
                            f.channels = e.length > 2 ? num(e[2]) : 1;
                        }
                    }
                    break;
                default: break;
            }
        }
        if (f.origin.isEmpty()) {
            f.origin = (b[4] & 0xFF) + "." + (b[5] & 0xFF) + "." + (b[6] & 0xFF) + "." + (b[7] & 0xFF);
        }
        return media && !f.name.isEmpty() ? f : null;
    }

    private static int num(String s) {
        try { return Integer.parseInt(s.trim()); } catch (NumberFormatException e) { return 0; }
    }
}
