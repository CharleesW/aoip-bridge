//! "Controller": roteamento entre as maquinas da rede, feito de qualquer painel.
//!
//! Cada programa AOIP anuncia a rede, uma vez por segundo, quem e (nome, IP), os canais que
//! envia (com os nomes, "Ch 01 Bumbo") e, para cada uma das suas entradas, de qual canal ela
//! esta recebendo. Qualquer painel mostra todas as maquinas e pode mandar outra maquina
//! ligar uma entrada dela a um canal de qualquer fluxo da rede.
//!
//! Na maquina que recebe, "entrada j <- canal c do fluxo F" vira: assinar o fluxo F numa das
//! posicoes de recepcao e ligar, na matriz de recepcao, o canal dele na saida j. Cada entrada
//! recebe de um canal so; um canal pode ir para quantas entradas quiser.
use crate::matrix::Matrix;
use crate::state::{Shared, RX_SLOTS};
use crate::MAX_FLOW_CHANNELS;
use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, TcpStream};
use std::time::{Duration, Instant};

/// Grupo e porta dos anuncios das maquinas.
pub const ANNOUNCE_GROUP: Ipv4Addr = Ipv4Addr::new(239, 255, 69, 1);
pub const ANNOUNCE_PORT: u16 = 8793;
/// Porta de comandos do AoIP Bridge; a placa virtual usa a seguinte.
pub const CONTROL_PORT: u16 = 8791;
/// Uma maquina some da lista se ficar este tempo sem anunciar.
pub const NODE_TIMEOUT: Duration = Duration::from_secs(6);
/// Tamanho maximo do nome de um canal (a parte depois de "Ch 01").
pub const LABEL_MAX: usize = 24;

/// Nome de um canal enviado: "Ch 01" mais o que o usuario escreveu ("Ch 01 Bumbo").
pub fn tx_label(index: usize, custom: &str) -> String {
    let custom = custom.trim();
    if custom.is_empty() { format!("Ch {:02}", index + 1) } else { format!("Ch {:02} {custom}", index + 1) }
}

/// Fluxo e canal (base 0) em que sai o canal `index` de uma maquina chamada `name`
/// (fluxos de 8 canais: "NOME : 1" leva os canais 1 a 8, e assim por diante).
pub fn flow_of_tx(name: &str, index: usize) -> (String, usize) {
    (format!("{} : {}", name, index / MAX_FLOW_CHANNELS + 1), index % MAX_FLOW_CHANNELS)
}

fn clean(s: &str, max: usize) -> String {
    s.chars().filter(|c| !c.is_control() && *c != '|').take(max).collect::<String>().trim().to_string()
}

// ---------------------------------------------------------------- rotas numa maquina

/// Liga a entrada `out` (saida da matriz de recepcao) ao canal `src.1` do fluxo `src.0`,
/// ou desliga (`None`). Usa uma posicao de recepcao livre se o fluxo ainda nao estiver
/// assinado, e solta a posicao que ficar sem uso.
pub fn set_route(slots: &mut [String], m: &mut Matrix, out: usize, src: Option<(&str, usize)>) -> Result<(), String> {
    if out >= m.outs() {
        return Err(format!("Esta maquina so tem {} entradas.", m.outs()));
    }
    if let Some((flow, ch)) = src {
        if flow.trim().is_empty() || ch >= MAX_FLOW_CHANNELS {
            return Err("Canal invalido.".into());
        }
    }
    let before = route_of(slots, m, out).map(|(s, _)| s);
    for i in 0..m.ins() {
        m.set(i, out, 0.0);
    }
    if let Some((flow, ch)) = src {
        let slot = match slots.iter().position(|s| s == flow) {
            Some(s) => s,
            None => {
                let free = (0..slots.len().min(RX_SLOTS)).find(|&s| slots[s].is_empty() && !slot_used(m, s))
                    .ok_or(format!("Esta maquina ja recebe {RX_SLOTS} fluxos diferentes (o maximo). Desligue entradas de outro fluxo primeiro."))?;
                slots[free] = flow.to_string();
                free
            }
        };
        m.set(slot * MAX_FLOW_CHANNELS + ch, out, 1.0);
    }
    // A posicao antiga ficou sem nenhuma entrada ligada: para de receber aquele fluxo.
    if let Some(old) = before.and_then(|f| slots.iter().position(|s| *s == f)) {
        if !slot_used(m, old) {
            slots[old].clear();
        }
    }
    Ok(())
}

fn slot_used(m: &Matrix, slot: usize) -> bool {
    let rows = slot * MAX_FLOW_CHANNELS..((slot + 1) * MAX_FLOW_CHANNELS).min(m.ins());
    rows.into_iter().any(|i| (0..m.outs()).any(|o| m.get(i, o) != 0.0))
}

/// De onde a entrada `out` recebe: (fluxo, canal base 0). Com mais de uma origem (mistura
/// feita na matriz), devolve a primeira.
pub fn route_of(slots: &[String], m: &Matrix, out: usize) -> Option<(String, usize)> {
    (0..m.ins()).find(|&i| m.get(i, out) != 0.0).and_then(|i| {
        let flow = slots.get(i / MAX_FLOW_CHANNELS)?;
        (!flow.is_empty()).then(|| (flow.clone(), i % MAX_FLOW_CHANNELS))
    })
}

/// Aplica uma rota nesta maquina (configuracao e matriz de recepcao) e grava.
pub fn apply_route(shared: &Shared, out: usize, src: Option<(&str, usize)>) -> Result<(), String> {
    let mut cfg = shared.config.lock().unwrap();
    let mut slots = cfg.rx.clone();
    let spec = shared.rx.edit(|m| set_route(&mut slots, m, out, src).map(|_| m.to_spec()))?;
    cfg.rx = slots;
    cfg.rx_map = Some(spec);
    drop(cfg);
    shared.save();
    Ok(())
}

/// Muda o nome de um canal enviado por esta maquina (a parte depois de "Ch 01") e grava.
pub fn apply_name(shared: &Shared, index: usize, custom: &str) -> Result<(), String> {
    if index >= crate::state::MAX_CH {
        return Err("Canal invalido.".into());
    }
    let mut cfg = shared.config.lock().unwrap();
    if cfg.tx_names.len() <= index {
        cfg.tx_names.resize(index + 1, String::new());
    }
    cfg.tx_names[index] = clean(custom, LABEL_MAX);
    drop(cfg);
    shared.save();
    Ok(())
}

// ---------------------------------------------------------------- anuncio das maquinas

/// Uma maquina da rede, como ela se anuncia.
#[derive(Clone, Debug, PartialEq)]
pub struct NodeInfo {
    /// "IP/instancia" (o AoIP Bridge e a placa virtual do mesmo computador sao duas maquinas).
    pub id: String,
    pub name: String,
    pub ip: Ipv4Addr,
    pub control: u16,
    pub version: String,
    /// Placa virtual (true) ou placa de verdade, ex.: a mesa (false).
    pub virtual_card: bool,
    /// Nome de cada canal enviado ("Ch 01 Bumbo").
    pub tx: Vec<String>,
    /// De onde recebe cada entrada (fluxo, canal base 0).
    pub routes: Vec<Option<(String, usize)>>,
}

impl NodeInfo {
    /// Monta o anuncio desta maquina a partir do estado dela.
    pub fn from_shared(shared: &Shared, id: String, ip: Ipv4Addr, control: u16) -> Self {
        let st = shared.status.lock().unwrap().clone();
        let cfg = shared.config.lock().unwrap().clone();
        let m = shared.rx.snapshot();
        let tx = (0..st.net_tx).map(|k| tx_label(k, cfg.tx_names.get(k).map_or("", |s| s.as_str()))).collect();
        let routes = (0..m.outs()).map(|o| route_of(&cfg.rx, &m, o)).collect();
        Self { id, name: st.name, ip, control, version: crate::VERSION.into(), virtual_card: st.virtual_card, tx, routes }
    }

    pub fn encode(&self) -> String {
        let mut t = format!(
            "AOIPNODE1\nid={}\nname={}\nip={}\nctl={}\nver={}\nvirtual={}\ntx={}\nrx={}\n",
            clean(&self.id, 40), clean(&self.name, 40), self.ip, self.control, clean(&self.version, 16),
            self.virtual_card as u8, self.tx.len(), self.routes.len()
        );
        for (k, label) in self.tx.iter().enumerate() {
            if *label != tx_label(k, "") {
                t += &format!("n{k}={}\n", clean(label, 40));
            }
        }
        for (j, r) in self.routes.iter().enumerate() {
            if let Some((flow, ch)) = r {
                t += &format!("r{j}={ch}|{}\n", clean(flow, 64));
            }
        }
        t
    }

    pub fn parse(text: &str) -> Option<Self> {
        let mut lines = text.lines();
        if lines.next()? != "AOIPNODE1" {
            return None;
        }
        let (mut id, mut name, mut ip, mut control, mut version, mut virt, mut ntx, mut nrx) =
            (None, String::new(), None, 0u16, String::new(), false, 0usize, 0usize);
        let (mut names, mut routes) = (Vec::new(), Vec::new());
        for line in lines {
            let Some((k, v)) = line.split_once('=') else { continue };
            match k {
                "id" => id = Some(v.to_string()),
                "name" => name = v.to_string(),
                "ip" => ip = v.parse().ok(),
                "ctl" => control = v.parse().unwrap_or(0),
                "ver" => version = v.to_string(),
                "virtual" => virt = v == "1",
                "tx" => ntx = v.parse::<usize>().unwrap_or(0).min(crate::state::MAX_CH),
                "rx" => nrx = v.parse::<usize>().unwrap_or(0).min(crate::state::MAX_CH),
                _ => {
                    if let Some(i) = k.strip_prefix('n').and_then(|i| i.parse::<usize>().ok()) {
                        names.push((i, v.to_string()));
                    } else if let Some(j) = k.strip_prefix('r').and_then(|j| j.parse::<usize>().ok()) {
                        if let Some((ch, flow)) = v.split_once('|') {
                            if let Ok(ch) = ch.parse::<usize>() {
                                routes.push((j, flow.to_string(), ch));
                            }
                        }
                    }
                }
            }
        }
        let mut tx: Vec<String> = (0..ntx).map(|k| tx_label(k, "")).collect();
        for (i, label) in names {
            if let Some(t) = tx.get_mut(i) {
                *t = label;
            }
        }
        let mut r: Vec<Option<(String, usize)>> = vec![None; nrx];
        for (j, flow, ch) in routes {
            if let Some(slot) = r.get_mut(j) {
                *slot = Some((flow, ch));
            }
        }
        Some(Self { id: id?, name, ip: ip?, control, version, virtual_card: virt, tx, routes: r })
    }
}

/// Uma maquina vista na rede, com a hora em que foi vista por ultimo.
#[derive(Clone, Debug)]
pub struct SeenNode {
    pub info: NodeInfo,
    pub seen: Instant,
    /// E esta propria maquina.
    pub own: bool,
}

/// Guarda (ou atualiza) uma maquina na lista e tira as que pararam de anunciar.
pub fn note_node(list: &mut Vec<SeenNode>, info: NodeInfo, own: bool, now: Instant) {
    list.retain(|n| n.info.id != info.id && now.duration_since(n.seen) < NODE_TIMEOUT);
    list.push(SeenNode { info, seen: now, own });
    list.sort_by(|a, b| (!a.own, a.info.name.to_lowercase()).cmp(&(!b.own, b.info.name.to_lowercase())));
}

// ---------------------------------------------------------------- comandos entre maquinas

/// Comando de uma linha: "ROUTE <entrada> <canal> <fluxo>", "UNROUTE <entrada>" ou
/// "NAME <canal> <nome>". Devolve a resposta ("OK" ou "ERR motivo").
pub fn handle_command(shared: &Shared, line: &str) -> String {
    let mut parts = line.trim_end_matches(['\r', '\n']).splitn(4, ' ');
    let result = match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some("ROUTE"), Some(j), Some(c), Some(flow)) => match (j.parse(), c.parse()) {
            (Ok(j), Ok(c)) => apply_route(shared, j, Some((flow, c))),
            _ => Err("comando invalido".into()),
        },
        (Some("UNROUTE"), Some(j), None, None) => j.parse().map_err(|_| "comando invalido".to_string()).and_then(|j| apply_route(shared, j, None)),
        (Some("NAME"), Some(k), rest1, rest2) => {
            let label = [rest1.unwrap_or(""), rest2.unwrap_or("")].join(" ");
            k.parse().map_err(|_| "comando invalido".to_string()).and_then(|k| apply_name(shared, k, &label))
        }
        _ => Err("comando desconhecido".into()),
    };
    match result {
        Ok(()) => "OK".into(),
        Err(e) => format!("ERR {e}"),
    }
}

/// Manda um comando para outra maquina e espera a resposta.
pub fn send_command(ip: Ipv4Addr, port: u16, command: &str) -> Result<(), String> {
    let addr = std::net::SocketAddr::from((ip, port));
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).map_err(|e| format!("Nao consegui falar com {ip} ({e})."))?;
    s.set_read_timeout(Some(Duration::from_secs(3))).ok();
    s.write_all(format!("{}\n", command.replace(['\r', '\n'], " ")).as_bytes()).map_err(|e| e.to_string())?;
    let mut reply = String::new();
    BufReader::new(s).read_line(&mut reply).map_err(|e| format!("{ip} nao respondeu ({e})."))?;
    match reply.trim_end().strip_prefix("ERR ") {
        Some(e) => Err(e.to_string()),
        None if reply.starts_with("OK") => Ok(()),
        None => Err(format!("{ip} respondeu algo inesperado.")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{Config, NET_RX};

    #[test]
    fn labels_and_flows() {
        assert_eq!(tx_label(0, ""), "Ch 01");
        assert_eq!(tx_label(4, " Bumbo "), "Ch 05 Bumbo");
        assert_eq!(tx_label(63, "Click"), "Ch 64 Click");
        assert_eq!(flow_of_tx("DESK", 0), ("DESK : 1".into(), 0));
        assert_eq!(flow_of_tx("DESK", 9), ("DESK : 2".into(), 1));
        assert_eq!(flow_of_tx("DESK", 31), ("DESK : 4".into(), 7));
    }

    #[test]
    fn routes_many_to_many() {
        // Maquina de Mix (32 entradas): 30 canais da mesa nas entradas 1-30 e 2 canais de
        // outra maquina nas entradas 31-32.
        let mut slots = vec![String::new(); RX_SLOTS];
        let mut m = Matrix::new(NET_RX, 32);
        for k in 0..30 {
            let (flow, ch) = flow_of_tx("DESK", k);
            set_route(&mut slots, &mut m, k, Some((&flow, ch))).unwrap();
        }
        set_route(&mut slots, &mut m, 30, Some(("NOTE Virtual : 1", 0))).unwrap();
        set_route(&mut slots, &mut m, 31, Some(("NOTE Virtual : 1", 1))).unwrap();
        assert_eq!(&slots[..5], ["DESK : 1", "DESK : 2", "DESK : 3", "DESK : 4", "NOTE Virtual : 1"]);
        assert_eq!(route_of(&slots, &m, 0), Some(("DESK : 1".into(), 0)));
        assert_eq!(route_of(&slots, &m, 29), Some(("DESK : 4".into(), 5)));
        assert_eq!(route_of(&slots, &m, 31), Some(("NOTE Virtual : 1".into(), 1)));
        // cada entrada recebe de um canal so: religar troca a origem
        set_route(&mut slots, &mut m, 0, Some(("DESK : 1", 3))).unwrap();
        assert_eq!(route_of(&slots, &m, 0), Some(("DESK : 1".into(), 3)));
        assert_eq!(m.points().iter().filter(|p| p.1 == 0).count(), 1);
        // desligar as duas entradas da outra maquina solta o fluxo dela
        set_route(&mut slots, &mut m, 30, None).unwrap();
        assert_eq!(slots[4], "NOTE Virtual : 1", "ainda tem a entrada 32");
        set_route(&mut slots, &mut m, 31, None).unwrap();
        assert_eq!(slots[4], "", "fluxo sem uso deixa de ser recebido");
        assert!(route_of(&slots, &m, 31).is_none());
        // um canal pode ir para varias entradas
        set_route(&mut slots, &mut m, 30, Some(("DESK : 1", 0))).unwrap();
        assert_eq!(route_of(&slots, &m, 30), route_of(&slots, &m, 1).map(|_| ("DESK : 1".into(), 0)));
        // limites
        assert!(set_route(&mut slots, &mut m, 32, None).is_err());
        let mut full: Vec<String> = (0..RX_SLOTS).map(|i| format!("F : {i}")).collect();
        let mut m2 = Matrix::new(NET_RX, 8);
        assert!(set_route(&mut full, &mut m2, 0, Some(("Outro : 1", 0))).unwrap_err().contains("8 fluxos"));
    }

    #[test]
    fn announcement_round_trip() {
        let n = NodeInfo {
            id: "192.168.5.49/0".into(), name: "DESK-IGREJA".into(), ip: "192.168.5.49".parse().unwrap(), control: 8791,
            version: "0.6.0".into(), virtual_card: false,
            tx: vec!["Ch 01 Bumbo".into(), "Ch 02".into(), "Ch 03 Caixa".into()],
            routes: vec![None, Some(("NOTE Virtual : 1".into(), 1)), None],
        };
        let text = n.encode();
        assert!(!text.contains("n1="), "nome padrao nao vai no anuncio");
        assert_eq!(NodeInfo::parse(&text), Some(n));
        assert_eq!(NodeInfo::parse("lixo"), None);
        assert_eq!(NodeInfo::parse("AOIPNODE1\nname=x\n"), None, "sem id e ip nao vale");
    }

    #[test]
    fn node_list_expires_and_orders() {
        let mk = |id: &str, name: &str| NodeInfo { id: id.into(), name: name.into(), ip: Ipv4Addr::LOCALHOST, control: 1, version: String::new(), virtual_card: false, tx: vec![], routes: vec![] };
        let t0 = Instant::now();
        let mut list = Vec::new();
        note_node(&mut list, mk("b", "Mix"), false, t0);
        note_node(&mut list, mk("a", "Gravacao"), false, t0);
        note_node(&mut list, mk("me", "Desk"), true, t0);
        assert_eq!(list.iter().map(|n| n.info.name.as_str()).collect::<Vec<_>>(), ["Desk", "Gravacao", "Mix"]);
        note_node(&mut list, mk("me", "Desk"), true, t0 + NODE_TIMEOUT + Duration::from_secs(1));
        assert_eq!(list.len(), 1, "quem parou de anunciar some");
    }

    #[test]
    fn commands_change_routes_and_names() {
        let s = Shared::new(Config::default(), std::path::PathBuf::from("/nao/existe/c.conf"));
        s.rx.edit(|m| *m = Matrix::new(NET_RX, 16));
        assert_eq!(handle_command(&s, "ROUTE 3 2 DESK : 1"), "OK");
        assert_eq!(s.config.lock().unwrap().rx[0], "DESK : 1");
        assert_eq!(route_of(&s.config.lock().unwrap().rx, &s.rx.snapshot(), 3), Some(("DESK : 1".into(), 2)));
        assert_eq!(handle_command(&s, "UNROUTE 3"), "OK");
        assert_eq!(s.config.lock().unwrap().rx[0], "");
        assert_eq!(handle_command(&s, "NAME 4 Bumbo kick"), "OK");
        assert_eq!(s.config.lock().unwrap().tx_names[4], "Bumbo kick");
        assert!(handle_command(&s, "ROUTE 99 0 X : 1").starts_with("ERR"));
        assert!(handle_command(&s, "APAGA TUDO").starts_with("ERR"));
    }
}
