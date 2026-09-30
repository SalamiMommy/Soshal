// ignore_for_file: invalid_use_of_internal_member
import 'dart:async';
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:just_audio_background/just_audio_background.dart';
import 'package:just_audio_media_kit/just_audio_media_kit.dart';
import 'package:media_kit/media_kit.dart';
import 'package:provider/provider.dart';
import 'routes/app_router.dart';
import 'services/ffi_bridge.dart';

import 'services/auth_service.dart';
import 'services/session_service.dart';
import 'services/settings_service.dart';
import 'services/sync_service.dart';
import 'services/messaging_service.dart' show IdentityService;
import 'services/notifications_service.dart';
import 'services/crypto_service.dart';
import 'services/network_service.dart';
import 'services/p2p_service.dart';
import 'services/ebpf_service.dart';
import 'services/error_log.dart';
import 'services/logging_scaffold_messenger.dart';
import 'services/telemetry_service.dart';
import 'services/layout_service.dart';
import 'services/turso_service.dart';
import 'services/shell_service.dart';
import 'services/signer_service.dart';
import 'services/theme_service.dart';
import 'services/analytics_service.dart';
import 'services/audit_service.dart';
import 'services/backup_service.dart';
import 'services/vouch_service.dart';
import 'services/daemon_service.dart';
import 'services/media_service.dart';
import 'services/mesh_service.dart';
import 'services/permissions_service.dart';
import 'services/profile_service.dart';
import 'utils/format.dart';

void main() async {
  WidgetsFlutterBinding.ensureInitialized();
  try {
    await JustAudioBackground.init(
      androidNotificationChannelId: 'com.soshal.audio',
      androidNotificationChannelName: 'Soshal Audio',
      androidNotificationIcon: 'mipmap/ic_launcher',
    );
  } catch (e) {
    debugPrint('JustAudioBackground init: $e');
  }
  JustAudioMediaKit.ensureInitialized();
  MediaKit.ensureInitialized();
  PaintingBinding.instance.imageCache.maximumSizeBytes = 50 * 1024 * 1024;
  PaintingBinding.instance.imageCache.maximumSize = 100;

  final telemetry = TelemetryService();
  TelemetryService.installCrashHooks(telemetry);
  runApp(
    MultiProvider(
      providers: [
        ChangeNotifierProvider<TelemetryService>(create: (_) => telemetry),
        ChangeNotifierProvider<LayoutService>(create: (_) => LayoutService()),
        ChangeNotifierProvider(create: (_) => AuthService()),
        ChangeNotifierProvider(create: (_) => SessionService()),
        ChangeNotifierProvider(create: (_) => SyncService()),
        ChangeNotifierProvider(create: (_) => IdentityService()),
        ChangeNotifierProvider(create: (_) => NetworkService()),
        ChangeNotifierProvider(
          create: (ctx) => MeshService(
            pubkey: () => ctx.read<SessionService>().activePubkey,
          ),
        ),
        ChangeNotifierProvider(create: (_) => ProfileService()),
        ChangeNotifierProvider(create: (_) => P2pService()),
        ChangeNotifierProvider(create: (_) => EbpfService()),
        ChangeNotifierProvider(create: (_) => TursoService()),
        ChangeNotifierProvider(create: (_) => ShellService()),
        ChangeNotifierProvider(create: (_) => SettingsService()),
        ChangeNotifierProvider(create: (_) => ThemeService()),
        ChangeNotifierProvider(create: (_) => AnalyticsService()),
        ChangeNotifierProvider(create: (_) => AuditService()),
        ChangeNotifierProvider(create: (_) => VouchService()),
        ChangeNotifierProvider(create: (_) => MediaService()),
        ChangeNotifierProvider(create: (_) => SignerService()),
        ChangeNotifierProvider(create: (_) => BackupService()),
        ChangeNotifierProvider(create: (_) => CryptoService()),
      ],
      child: const SoshalApp(),
    ),
  );
}

class SoshalApp extends StatefulWidget {
  const SoshalApp({super.key});

  @override
  State<SoshalApp> createState() => _SoshalAppState();
}

class _SoshalAppState extends State<SoshalApp> {
  bool _restartSyncOnUnlock = false;
  List<String>? _syncRelays;

  @override
  void initState() {
    super.initState();
    _initializeFfiBridge();
  }

  Future<void> _initializeFfiBridge() async {
    try {
      // Init the bridge + shared settings DB (migrations included) before
      // any service touches it — shell/theme/settings reads fail otherwise
      // ("database not initialized").
      await FfiBridge.ensureDatabaseInitialized();
      if (!mounted) return;
      // Bring up bundled networking daemons (i2pd, freenet, rnsd) so the
      // local transports are live before any screen needs them. No-op on
      // desktop (no bundled assets) and when already running.
      // Android 13+: foreground-service notification needs the runtime
      // permission to be visible (service runs regardless). Fire once on
      // startup when the daemons are coming up.
      unawaited(() async {
        try {
          await DaemonService.startDaemons();
        } catch (e, st) {
          logRuntimeError('startDaemons: $e\n$st');
        }
      }());
      unawaited(() async {
        try {
          await PermissionsService.ensureNotifications();
        } catch (e, st) {
          logRuntimeError('ensureNotifications: $e\n$st');
        }
      }());
      unawaited(() async {
        try {
          await NotificationService.initLocalNotifications();
        } catch (e, st) {
          logRuntimeError('initLocalNotifications: $e\n$st');
        }
      }());
      context.read<TelemetryService>().init();
      context.read<ShellService>().initialize();
      context.read<ThemeService>().load();
      // Bring up the shared relay client before any screen touches it, so
      // relay-gated fetches (chatrandom, musicloud, …) don't fail with
      // "relay client not initialized" before sign-in. Idempotent; account
      // relays take over after auth. A failure here (e.g. boot fully offline)
      // must NOT abort the rest of the startup wiring — relays are retried
      // lazily by consuming services and the client re-inits on connect.
      final networkService = context.read<NetworkService>();
      try {
        await networkService.initRelays(NetworkService.defaultRelays);
      } catch (e, st) {
        logRuntimeError('initRelays (retried lazily): $e\n$st');
      }
      if (!mounted) return;
      final syncService = context.read<SyncService>();
      syncService.attach(
        p2p: context.read<P2pService>(),
      );
      context.read<SessionService>().attachSync(context.read<SyncService>());
      final signerService = context.read<SignerService>();
      signerService.onLock = () {
        if (syncService.started) {
          _restartSyncOnUnlock = true;
          _syncRelays = context.read<SessionService>().activeAccount?.relayList;
          unawaited(syncService.stop());
        }
      };
      signerService.onUnlock = () {
        final session = context.read<SessionService>();
        if (_restartSyncOnUnlock && session.hasActiveSession()) {
          _restartSyncOnUnlock = false;
          unawaited(syncService.start(
            relays: (_syncRelays?.isNotEmpty ?? false)
                ? _syncRelays!
                : const ['wss://relay.nostr.band', 'wss://nos.lol'],
          ));
        }
      };
      context.read<SessionService>().attachAccountScopedServices(
            p2p: context.read<P2pService>(),
            network: context.read<NetworkService>(),
            turso: context.read<TursoService>(),
            shell: context.read<ShellService>(),
          );
      // Tell the Rust network stack we came up on Wi-Fi. Resolves the real
      // local IP + QUIC port; no-ops silently when unavailable.
      await networkService.notifyInterfaceChange();
      // Defensive initial deep-link attempt — no plugin installed, so the
      // default route is parsed as a potential nostr: URI. Failures are
      // swallowed; the protocol handler stays wired for when a real link
      // arrives.
      try {
        final route = ui.PlatformDispatcher.instance.defaultRouteName;
        final uri = Uri.tryParse(route);
        if (uri == null || uri.scheme.isEmpty) {
          // No deep link.
        } else if (uri.scheme == 'nostr') {
          await _handleNostrDeepLink(route);
        } else if (uri.scheme == 'app') {
          // Allowlisted scheme only; anything else is dropped (Rust side
          // rejects unknown schemes too, this keeps the Dart heap quiet).
          if (!mounted) return;
          await context.read<AuthService>().handleNostrProtocolRequest(
                scheme: uri.scheme,
                host: uri.host,
                path: uri.path,
              );
        }
      } catch (e) {
        debugPrint('initial deep link: $e');
      }
    } catch (e, st) {
      logRuntimeError('Error initializing FFI bridge: $e', st);
    }
  }

  /// Routes a `nostr:` URI to the matching screen: `npub1`/`nprofile1` →
  /// profile, `note1`/`nevent1` → post. Unknown or malformed payloads are
  /// ignored silently.
  Future<void> _handleNostrDeepLink(String raw) async {
    final rest = raw.startsWith('nostr:') ? raw.substring(6) : raw;
    final split = rest.indexOf('1');
    if (split <= 0) return;
    final hrp = rest.substring(0, split);
    switch (hrp) {
      case 'npub':
      case 'nprofile':
        final hex = await context.read<AuthService>().decodeNpub(rest);
        if (hex.isEmpty) return;
        await AppRouter.router.push('/profile/$hex');
      case 'note':
      case 'nevent':
        final data = _bech32Decode(rest);
        if (data == null || data.length < 32) return;
        final hex = bytesToHex(data.sublist(0, 32));
        await AppRouter.router.push('/post/$hex');
    }
  }

  /// Minimal bech32 decoder (NIP-19 payloads): converts the 5-bit data part
  /// back to bytes. No checksum validation — only payloads that decode to at
  /// least 32 bytes are used.
  static List<int>? _bech32Decode(String input) {
    const charset = 'qpzry9x8gf2tvdw0s3jn54khce6mua7l';
    final lower = input.toLowerCase();
    final pos = lower.lastIndexOf('1');
    if (pos < 1) return null;
    final dataPart = lower.substring(pos + 1);
    if (dataPart.isEmpty || dataPart.length > 90) return null;
    var acc = 0;
    var bits = 0;
    final out = <int>[];
    for (final code in dataPart.codeUnits) {
      final v = charset.indexOf(String.fromCharCode(code));
      if (v < 0) return null;
      acc = (acc << 5) | v;
      bits += 5;
      if (bits >= 8) {
        bits -= 8;
        out.add((acc >> bits) & 0xff);
      }
    }
    return out;
  }

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: context.read<ThemeService>(),
      builder: (context, _) {
        final themeService = context.read<ThemeService>();
        return MaterialApp.router(
          title: 'Soshal',
          theme: themeService.themeFor(),
          darkTheme: themeService.themeFor(),
          themeMode: themeService.isDark ? ThemeMode.dark : ThemeMode.light,
          builder: (context, child) => LoggingScaffoldMessenger(
            // Opaque fallback behind the transparent shell scaffolds:
            // splash/auth paint over this; in-shell screens paint over the
            // AppShell background image instead.
            child: ColoredBox(
              color: themeService.isDark
                  ? const Color(0xFF05070A)
                  : const Color(0xFFF0F7FC),
              child: child ?? const SizedBox.shrink(),
            ),
          ),
          routerConfig: AppRouter.router,
        );
      },
    );
  }
}
