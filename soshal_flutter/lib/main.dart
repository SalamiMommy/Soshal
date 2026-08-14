// ignore_for_file: invalid_use_of_internal_member
import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import 'routes/app_router.dart';
import 'services/ffi_bridge.dart';
import 'ffi/db.dart' as ffi_db;
import 'services/auth_service.dart';
import 'services/feed_service.dart';
import 'services/session_service.dart';
import 'services/sync_service.dart';
import 'services/messaging_service.dart';
import 'services/notifications_service.dart';
import 'services/search_service.dart';
import 'services/dating_service.dart';
import 'services/events_service.dart';
import 'services/groups_service.dart';
import 'services/marketplace_service.dart';
import 'services/zap_service.dart';
import 'services/streaming_service.dart';
import 'services/network_service.dart';
import 'services/p2p_service.dart';
import 'services/moderation_service.dart';
import 'services/ebpf_service.dart';
import 'services/logging_scaffold_messenger.dart';
import 'services/telemetry_service.dart';
import 'services/layout_service.dart';
import 'services/turso_service.dart';
import 'services/shell_service.dart';
import 'services/theme_service.dart';
import 'services/music_service.dart';
import 'services/friends_service.dart';
import 'services/minis_service.dart';
import 'services/analytics_service.dart';
import 'services/audit_service.dart';
import 'services/bookmarks_service.dart';
import 'services/scheduled_service.dart';
import 'services/stealth_service.dart';
import 'services/vouch_service.dart';
import 'services/calls_service.dart';
import 'services/chatrandom_service.dart';
import 'services/media_service.dart';
import 'services/mesh_service.dart';
import 'services/nostr_service.dart';

void main() {
  WidgetsFlutterBinding.ensureInitialized();
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
        ChangeNotifierProvider(create: (_) => FeedService()),
        ChangeNotifierProvider(create: (_) => SessionService()),
        ChangeNotifierProvider(create: (_) => SyncService()),
        ChangeNotifierProvider(create: (_) => MessagingService()),
        ChangeNotifierProvider(create: (_) => IdentityService()),
        ChangeNotifierProvider(create: (_) => NotificationService()),
        ChangeNotifierProvider(create: (_) => SearchService()),
        ChangeNotifierProvider(create: (_) => DatingService()),
        ChangeNotifierProvider(create: (_) => EventsService()),
        ChangeNotifierProvider(create: (_) => GroupsService()),
        ChangeNotifierProvider(create: (_) => MarketplaceService()),
        ChangeNotifierProvider(create: (_) => ZapService()),
        ChangeNotifierProvider(create: (_) => StreamingService()),
        ChangeNotifierProvider(create: (_) => NetworkService()),
        ChangeNotifierProvider(
          create: (ctx) => MeshService(
            pubkey: () => ctx.read<SessionService>().activePubkey,
          ),
        ),
        ChangeNotifierProvider(create: (_) => NostrService()),
        ChangeNotifierProvider(create: (_) => P2pService()),
        ChangeNotifierProvider(create: (_) => ModerationService()),
        ChangeNotifierProvider(create: (_) => EbpfService()),
        ChangeNotifierProvider(create: (_) => TursoService()),
        ChangeNotifierProvider(create: (_) => ShellService()),
        ChangeNotifierProvider(create: (_) => ThemeService()),
        ChangeNotifierProvider(create: (_) => MusicService()),
        ChangeNotifierProvider(create: (_) => FriendsService()),
        ChangeNotifierProvider(create: (_) => AnalyticsService()),
        ChangeNotifierProvider(create: (_) => AuditService()),
        ChangeNotifierProvider(create: (_) => BookmarksService()),
        ChangeNotifierProvider(create: (_) => ScheduledService()),
        ChangeNotifierProvider(create: (_) => StealthService()),
        ChangeNotifierProvider(create: (_) => VouchService()),
        ChangeNotifierProvider(create: (_) => CallsService()),
        ChangeNotifierProvider(create: (_) => ChatrandomService()),
        ChangeNotifierProvider(create: (_) => MediaService()),
        Provider<MinisService>(create: (_) => MinisService()),
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
  @override
  void initState() {
    super.initState();
    _initializeFfiBridge();
  }

  Future<void> _initializeFfiBridge() async {
    try {
      await FfiBridge.init();
      // Open the shared settings DB (migrations included) before any
      // service touches it — shell/theme/settings reads fail otherwise
      // ("database not initialized").
      ffi_db.dbInit(dbPath: await FfiBridge.getDbPath());
      if (!mounted) return;
      context.read<TelemetryService>().init();
      context.read<ShellService>().initialize();
      context.read<ThemeService>().load();
      final syncService = context.read<SyncService>();
      syncService.attach(
        feed: context.read<FeedService>(),
        messaging: context.read<MessagingService>(),
      );
    } catch (e) {
      debugPrint('Error initializing FFI bridge: $e');
    }
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
          builder: (context, child) =>
              LoggingScaffoldMessenger(child: child ?? const SizedBox.shrink()),
          routerConfig: AppRouter.router,
        );
      },
    );
  }
}
