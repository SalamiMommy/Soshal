import 'dart:async';

import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';
import '../services/ffi_bridge.dart';
import '../services/session_service.dart';
import '../services/signer_service.dart';
import '../services/sync_service.dart';
import '../services/error_log.dart';

/// Splash Screen
/// Initialization, load DB, check session
class SplashScreen extends StatefulWidget {
  const SplashScreen({super.key});

  @override
  State<SplashScreen> createState() => _SplashScreenState();
}

class _SplashScreenState extends State<SplashScreen> {
  @override
  void initState() {
    super.initState();
    _initializeApp();
  }

  Future<void> _initializeApp() async {
    try {
      // Initialize FFI bridge
      await FfiBridge.init();
      if (!mounted) return;

      // Load session
      final sessionService = context.read<SessionService>();
      final signer = context.read<SignerService>();
      await sessionService.loadSession();
      await signer.refresh();

      // Check if user is logged in
      if (sessionService.hasActiveSession()) {
        // A session without loaded keys (never unlocked this run) routes to
        // onboarding for re-auth — the lock screen only appears after an
        // explicit in-session "Lock now".
        if (signer.locked && !signer.userLocked) {
          if (mounted) {
            context.go('/auth');
          }
          return;
        }
        // Start the Rust-side background relay sync (feed/messages ingest).
        final relays = sessionService.activeAccount?.relayList ?? <String>[];
        if (relays.isNotEmpty && mounted) {
          unawaited(context.read<SyncService>().start(relays: relays));
        }
        // Route to feed
        if (mounted) {
          context.go('/feed');
        }
      } else {
        // Route to auth
        if (mounted) {
          context.go('/auth');
        }
      }
    } catch (e, st) {
      logRuntimeError(e, st);
      // Handle initialization error
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: SelectableText('Initialization error: $e')),
        );
        context.go('/auth');
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    return const Scaffold(
      body: Center(
        child: Column(
          mainAxisAlignment: MainAxisAlignment.center,
          children: [
            Text(
              'Soshal',
              style: TextStyle(fontSize: 32, fontWeight: FontWeight.bold),
            ),
            SizedBox(height: 16),
            CircularProgressIndicator(),
            SizedBox(height: 16),
            Text('Loading...'),
          ],
        ),
      ),
    );
  }
}
