import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';
import '../services/session_service.dart';
import '../services/signer_service.dart';

/// Full-screen signer lock overlay: shown when a session exists but the
/// in-process signer holds no keys (restart or explicit "Lock now").
/// Unlocks the locally stored profile via the local keystore/keychain.
class SignerLockScreen extends StatefulWidget {
  const SignerLockScreen({super.key});

  @override
  State<SignerLockScreen> createState() => _SignerLockScreenState();
}

class _SignerLockScreenState extends State<SignerLockScreen> {
  bool _busy = false;
  String? _error;

  Future<void> _unlockProfile() async {
    final signer = context.read<SignerService>();
    final session = context.read<SessionService>();
    final pubkey = session.activePubkey;
    if (pubkey == null || _busy) return;
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final ok = await signer.unlockFromKeyring(pubkey);
      if (!ok && mounted) {
        setState(() => _error = 'Could not unlock local profile');
      }
    } catch (e) {
      if (mounted) setState(() => _error = '$e');
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _showSecretKeyDialog() async {
    final controller = TextEditingController();
    final result = await showDialog<String>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: const Text('Unlock with Key or Phrase'),
        content: TextField(
          controller: controller,
          decoration: const InputDecoration(
            hintText: 'Enter nsec or recovery phrase',
            border: OutlineInputBorder(),
          ),
          autofocus: true,
          obscureText: true,
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(ctx).pop(),
            child: const Text('Cancel'),
          ),
          FilledButton(
            onPressed: () => Navigator.of(ctx).pop(controller.text.trim()),
            child: const Text('Unlock'),
          ),
        ],
      ),
    );
    if (result != null && result.isNotEmpty && mounted) {
      setState(() {
        _busy = true;
        _error = null;
      });
      try {
        final signer = context.read<SignerService>();
        await signer.unlock(result);
      } catch (e) {
        if (mounted) setState(() => _error = '$e');
      } finally {
        if (mounted) setState(() => _busy = false);
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);

    return Scaffold(
      backgroundColor: theme.colorScheme.surface,
      body: SafeArea(
        child: Center(
          child: SingleChildScrollView(
            padding: const EdgeInsets.all(24),
            child: ConstrainedBox(
              constraints: const BoxConstraints(maxWidth: 420),
              child: Column(
                mainAxisAlignment: MainAxisAlignment.center,
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Icon(
                    Icons.lock_outline,
                    size: 56,
                    color: theme.colorScheme.primary,
                  ),
                  const SizedBox(height: 16),
                  Text(
                    'Profile locked',
                    textAlign: TextAlign.center,
                    style: theme.textTheme.headlineSmall,
                  ),
                  const SizedBox(height: 8),
                  Text(
                    'Your session is locked. Unlock your locally stored profile '
                    'to post, send messages, and sign events.',
                    textAlign: TextAlign.center,
                    style: theme.textTheme.bodyMedium,
                  ),
                  const SizedBox(height: 24),
                  FilledButton.icon(
                    onPressed: _busy ? null : _unlockProfile,
                    icon: const Icon(Icons.lock_open),
                    label: Text(_busy ? 'Unlocking…' : 'Unlock Profile'),
                  ),
                  const SizedBox(height: 8),
                  TextButton.icon(
                    onPressed: _busy ? null : _showSecretKeyDialog,
                    icon: const Icon(Icons.key),
                    label: const Text('Unlock with key or phrase'),
                  ),
                  const SizedBox(height: 8),
                  TextButton(
                    onPressed: () => context.go('/auth'),
                    child: const Text('Switch or import account'),
                  ),
                  if (_error != null) ...[
                    const SizedBox(height: 12),
                    Text(
                      _error!,
                      textAlign: TextAlign.center,
                      style:
                          TextStyle(color: Theme.of(context).colorScheme.error),
                    ),
                  ],
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }
}
