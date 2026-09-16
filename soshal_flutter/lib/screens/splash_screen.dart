import 'dart:async';

import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';
import '../services/session_service.dart';
import '../services/settings_service.dart';
import '../services/shell_service.dart';
import '../services/signer_service.dart';
import '../services/sync_service.dart';
import '../services/ffi_bridge.dart';
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
      if (!mounted) return;

      // Serialize behind the app-root init: every DB read below must wait
      // for db_init or the global handle is still None ("database not
      // initialized" race).
      await FfiBridge.ensureDatabaseInitialized();
      if (!mounted) return;

      // Load session
      final sessionService = context.read<SessionService>();
      final signer = context.read<SignerService>();
      final shell = context.read<ShellService>();
      final autologinEnabled =
          context.read<SettingsService>().getSetting('autologin_enabled') !=
              'false';
      await Future.wait([
        sessionService.loadSession(),
        shell.initialize(),
        signer.refresh(),
      ]);

      // Check if user is logged in
      final hasAccounts = sessionService.getAccounts().isNotEmpty;
      if (sessionService.hasActiveSession() || hasAccounts) {
        // Autologin setting: when off, require explicit login even if a
        // session exists.
        if (!autologinEnabled) {
          if (mounted) {
            context.go('/auth');
          }
          return;
        }

        if (sessionService.activePubkey == null && hasAccounts) {
          final lastUsed = sessionService.lastUsedAccount;
          if (lastUsed != null) {
            await sessionService.switchAccount(lastUsed.pubkey);
          }
        }

        // Persistence: a session with no loaded keys auto-unlocks from the
        // local keystore/keychain when no PIN is configured and keychain unlock is
        // enabled (defaults to true).
        if (!mounted) return;
        final keychainUnlockEnabled = context
                .read<SettingsService>()
                .getSetting('keychain_unlock_enabled') !=
            'false';
        if (signer.locked && !shell.hasPin && keychainUnlockEnabled) {
          final activePubkey = sessionService.activePubkey;
          if (activePubkey != null) {
            try {
              await signer.unlockFromKeyring(activePubkey);
            } catch (e, st) {
              logRuntimeError('keyring unlock: $e', st);
            }
          }
        }
        // If the signer is still locked and no PIN is configured (keychain
        // unlock failed or was disabled), route to /auth to re-authenticate.
        // If a PIN is configured, proceed to /feed where LockScreen prompts
        // for the PIN to unlock the signer.
        if (signer.locked && !shell.hasPin) {
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
      // Handle initialization error. The catch can run synchronously inside
      // initState (when the pre-first-await segment of _initializeApp throws),
      // so defer the ScaffoldMessenger lookup past the current frame.
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!mounted) return;
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: SelectableText('Initialization error: $e')),
        );
        context.go('/auth');
      });
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
