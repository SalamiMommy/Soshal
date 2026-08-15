import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import '../services/auth_service.dart';
import '../services/session_service.dart';
import '../services/signer_service.dart';

/// Full-screen signer lock overlay: shown when a session exists but the
/// in-process signer holds no keys (restart without keychain, or explicit
/// "Lock now"). Unlock via OS keychain or recovery phrase.
class SignerLockScreen extends StatefulWidget {
  const SignerLockScreen({super.key});

  @override
  State<SignerLockScreen> createState() => _SignerLockScreenState();
}

class _SignerLockScreenState extends State<SignerLockScreen> {
  final TextEditingController _phrase = TextEditingController();
  bool _busy = false;
  String? _error;

  Future<void> _unlockFromKeychain() async {
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
        setState(() => _error = 'No matching key found in the device keychain');
      }
    } catch (e) {
      if (mounted) setState(() => _error = '$e');
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _unlockWithPhrase() async {
    final phrase = _phrase.text.trim();
    if (phrase.isEmpty || _busy) return;
    final auth = context.read<AuthService>();
    final signer = context.read<SignerService>();
    final session = context.read<SessionService>();
    final activePubkey = session.activePubkey;
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final keypair = await auth.restoreFromMnemonic(phrase, '');
      if (activePubkey != null && keypair.publicKey != activePubkey) {
        setState(() => _error =
            'Recovery phrase does not match the active account');
      } else {
        await signer.refresh();
      }
    } catch (e) {
      if (mounted) setState(() => _error = '$e');
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  void dispose() {
    _phrase.dispose();
    super.dispose();
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
                    Icons.key_off_outlined,
                    size: 56,
                    color: theme.colorScheme.primary,
                  ),
                  const SizedBox(height: 16),
                  Text(
                    'Signer locked',
                    textAlign: TextAlign.center,
                    style: theme.textTheme.headlineSmall,
                  ),
                  const SizedBox(height: 8),
                  Text(
                    'Your keys are wiped for this session. Unlock with the '
                    'device keychain or your recovery phrase to post, send '
                    'messages, and sign events.',
                    textAlign: TextAlign.center,
                    style: theme.textTheme.bodyMedium,
                  ),
                  const SizedBox(height: 24),
                  FilledButton.icon(
                    onPressed: _busy ? null : _unlockFromKeychain,
                    icon: const Icon(Icons.key),
                    label: const Text('Unlock from device keychain'),
                  ),
                  const SizedBox(height: 24),
                  Text(
                    'or restore with recovery phrase',
                    textAlign: TextAlign.center,
                    style: theme.textTheme.bodySmall,
                  ),
                  const SizedBox(height: 8),
                  TextField(
                    controller: _phrase,
                    enabled: !_busy,
                    maxLines: 3,
                    decoration: const InputDecoration(
                      labelText: 'Recovery phrase',
                      border: OutlineInputBorder(),
                    ),
                  ),
                  const SizedBox(height: 12),
                  FilledButton(
                    onPressed: _busy ? null : _unlockWithPhrase,
                    child: Text(_busy ? 'Unlocking…' : 'Unlock'),
                  ),
                  if (_error != null) ...[
                    const SizedBox(height: 12),
                    Text(
                      _error!,
                      textAlign: TextAlign.center,
                      style: const TextStyle(color: Colors.redAccent),
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