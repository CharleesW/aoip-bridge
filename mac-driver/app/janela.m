// Janela do AOIP-Bridge no Mac: mostra a tela do programa (servida por aoip-virtual em
// 127.0.0.1:8788) numa janela comum, sem navegador. O audio NAO passa por aqui: fechar a
// janela nao para nada.
#import <Cocoa/Cocoa.h>
#import <WebKit/WebKit.h>

static NSURL *appURL(void) {
    const char *p = getenv("AOIP_PANEL_PORT"); // para teste
    int port = p ? atoi(p) : 8788;
    return [NSURL URLWithString:[NSString stringWithFormat:@"http://127.0.0.1:%d/app?virtual=%d", port, port]];
}

@interface Janela : NSObject <NSApplicationDelegate, WKNavigationDelegate, WKUIDelegate>
@property(strong) NSWindow *window;
@property(strong) WKWebView *web;
@property(strong) NSTextField *aviso;
@end

@implementation Janela

- (void)applicationDidFinishLaunching:(NSNotification *)n {
    [self menu];
    NSRect r = NSMakeRect(0, 0, 1240, 820);
    self.window = [[NSWindow alloc] initWithContentRect:r
        styleMask:NSWindowStyleMaskTitled | NSWindowStyleMaskClosable | NSWindowStyleMaskMiniaturizable | NSWindowStyleMaskResizable
        backing:NSBackingStoreBuffered defer:NO];
    self.window.title = @"AOIP-Bridge";
    self.window.releasedWhenClosed = NO;
    self.window.minSize = NSMakeSize(760, 520);
    self.web = [[WKWebView alloc] initWithFrame:r configuration:[WKWebViewConfiguration new]];
    self.web.navigationDelegate = self;
    self.web.UIDelegate = self;
    self.web.autoresizingMask = NSViewWidthSizable | NSViewHeightSizable;
    self.web.hidden = YES;
    NSView *fundo = [[NSView alloc] initWithFrame:r];
    self.aviso = [NSTextField wrappingLabelWithString:@"Procurando o AOIP-Bridge neste Mac…"];
    self.aviso.alignment = NSTextAlignmentCenter;
    self.aviso.frame = NSMakeRect(320, 360, 600, 100);
    self.aviso.autoresizingMask = NSViewMinXMargin | NSViewMaxXMargin | NSViewMinYMargin | NSViewMaxYMargin;
    [fundo addSubview:self.aviso];
    [fundo addSubview:self.web];
    self.window.contentView = fundo;
    [self.window center];
    [self.window setFrameAutosaveName:@"principal"];
    [self.window makeKeyAndOrderFront:nil];
    [NSApp activateIgnoringOtherApps:YES];
    [self abrir];
}

- (void)abrir {
    NSURLRequest *q = [NSURLRequest requestWithURL:appURL() cachePolicy:NSURLRequestReloadIgnoringLocalCacheData timeoutInterval:5];
    [self.web loadRequest:q];
}

- (void)falhou {
    self.web.hidden = YES;
    self.aviso.stringValue = @"O AOIP-Bridge não está respondendo neste Mac. Tentando de novo…\n\nSe continuar assim, rode o Instalar.command outra vez.";
    dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 2 * NSEC_PER_SEC), dispatch_get_main_queue(), ^{ [self abrir]; });
}

// ---- carregamento
- (void)webView:(WKWebView *)w didFinishNavigation:(WKNavigation *)n { self.web.hidden = NO; }
- (void)webView:(WKWebView *)w didFailProvisionalNavigation:(WKNavigation *)n withError:(NSError *)e {
    if (e.code == NSURLErrorCancelled) return;
    NSLog(@"AOIP-Bridge: %@", e.localizedDescription);
    [self falhou];
}
- (void)webView:(WKWebView *)w didFailNavigation:(WKNavigation *)n withError:(NSError *)e {
    if (e.code != NSURLErrorCancelled) [self falhou];
}
- (void)webViewWebContentProcessDidTerminate:(WKWebView *)w { [self abrir]; }

// Enderecos de fora (pagina da versao no GitHub) abrem no navegador; aqui so o proprio programa.
- (void)webView:(WKWebView *)w decidePolicyForNavigationAction:(WKNavigationAction *)a decisionHandler:(void (^)(WKNavigationActionPolicy))ok {
    NSURL *u = a.request.URL;
    BOOL nosso = [u.host isEqualToString:@"127.0.0.1"] || [u.scheme isEqualToString:@"about"];
    if (nosso && a.targetFrame) { ok(WKNavigationActionPolicyAllow); return; }
    if (!nosso && a.navigationType == WKNavigationTypeLinkActivated && [u.scheme hasPrefix:@"http"]) [[NSWorkspace sharedWorkspace] openURL:u];
    ok(WKNavigationActionPolicyCancel);
}

// ---- perguntas da pagina (sem isto o WebKit as ignora)
- (NSAlert *)alerta:(NSString *)m {
    NSAlert *a = [NSAlert new];
    a.messageText = @"AOIP-Bridge";
    a.informativeText = m;
    [a addButtonWithTitle:@"OK"];
    return a;
}
- (void)webView:(WKWebView *)w runJavaScriptAlertPanelWithMessage:(NSString *)m initiatedByFrame:(WKFrameInfo *)f completionHandler:(void (^)(void))ok {
    [[self alerta:m] beginSheetModalForWindow:self.window completionHandler:^(NSModalResponse r) { ok(); }];
}
- (void)webView:(WKWebView *)w runJavaScriptConfirmPanelWithMessage:(NSString *)m initiatedByFrame:(WKFrameInfo *)f completionHandler:(void (^)(BOOL))ok {
    NSAlert *a = [self alerta:m];
    [a addButtonWithTitle:@"Cancelar"];
    [a beginSheetModalForWindow:self.window completionHandler:^(NSModalResponse r) { ok(r == NSAlertFirstButtonReturn); }];
}
- (void)webView:(WKWebView *)w runJavaScriptTextInputPanelWithPrompt:(NSString *)m defaultText:(NSString *)d initiatedByFrame:(WKFrameInfo *)f completionHandler:(void (^)(NSString *))ok {
    NSAlert *a = [self alerta:m];
    [a addButtonWithTitle:@"Cancelar"];
    NSTextField *t = [[NSTextField alloc] initWithFrame:NSMakeRect(0, 0, 300, 24)];
    t.stringValue = d ?: @"";
    a.accessoryView = t;
    a.window.initialFirstResponder = t;
    [a beginSheetModalForWindow:self.window completionHandler:^(NSModalResponse r) { ok(r == NSAlertFirstButtonReturn ? t.stringValue : nil); }];
}

// ---- programa
- (BOOL)applicationShouldTerminateAfterLastWindowClosed:(NSApplication *)a { return YES; }
- (BOOL)applicationSupportsSecureRestorableState:(NSApplication *)a { return YES; }
- (void)recarregar:(id)s { [self abrir]; }

- (void)menu {
    NSMenu *bar = [NSMenu new];
    NSMenu *(^sub)(NSString *) = ^(NSString *t) {
        NSMenuItem *i = [bar addItemWithTitle:t action:nil keyEquivalent:@""];
        return (i.submenu = [[NSMenu alloc] initWithTitle:t]);
    };
    NSMenu *app = sub(@"AOIP-Bridge");
    [app addItemWithTitle:@"Sobre o AOIP-Bridge" action:@selector(orderFrontStandardAboutPanel:) keyEquivalent:@""];
    [app addItem:[NSMenuItem separatorItem]];
    [app addItemWithTitle:@"Fechar a janela (o áudio continua)" action:@selector(terminate:) keyEquivalent:@"q"];
    NSMenu *ed = sub(@"Editar");
    [ed addItemWithTitle:@"Desfazer" action:@selector(undo:) keyEquivalent:@"z"];
    [ed addItemWithTitle:@"Recortar" action:@selector(cut:) keyEquivalent:@"x"];
    [ed addItemWithTitle:@"Copiar" action:@selector(copy:) keyEquivalent:@"c"];
    [ed addItemWithTitle:@"Colar" action:@selector(paste:) keyEquivalent:@"v"];
    [ed addItemWithTitle:@"Selecionar tudo" action:@selector(selectAll:) keyEquivalent:@"a"];
    NSMenu *win = sub(@"Janela");
    [win addItemWithTitle:@"Recarregar" action:@selector(recarregar:) keyEquivalent:@"r"].target = self;
    [win addItemWithTitle:@"Minimizar" action:@selector(performMiniaturize:) keyEquivalent:@"m"];
    [win addItemWithTitle:@"Fechar" action:@selector(performClose:) keyEquivalent:@"w"];
    NSApp.mainMenu = bar;
    NSApp.windowsMenu = win;
}
@end

int main(void) {
    @autoreleasepool {
        [NSApplication sharedApplication];
        [NSApp setActivationPolicy:NSApplicationActivationPolicyRegular];
        Janela *j = [Janela new];
        NSApp.delegate = j;
        [NSApp run];
    }
    return 0;
}
