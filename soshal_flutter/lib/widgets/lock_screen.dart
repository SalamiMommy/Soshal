import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import '../services/session_service.dart';
import '../services/shell_service.dart';
import '../services/signer_service.dart';

/// Full-screen PIN lock overlay (mirrors the legacy rust-native lock screen).
class LockScreen extends StatefulWidget {
  const LockScreen({super.key});

  @override
  State<LockScreen> createState() => _LockScreenState();
}

class _LockScreenState extends State<LockScreen> {
  final TextEditingController _pin = TextEditingController();

  @override
  void initState() {
    super.initState();
    _maybeBiometric();
  }

  Future<void> _maybeBiometric() async {
    // Biometric prompt support is platform-FFI wired later; keep the lock
    // PIN-first and honest about the biometric setting.
  }

  void _submit(ShellService shell) {
    final pin = _pin.text;
    if (pin.isEmpty) return;
    _unlock(shell, pin);
    _pin.clear();
  }

  Future<void> _unlock(ShellService shell, String pin) async {
    final ok = await shell.unlock(pin);
    if (!ok || !mounted) return;
    // PIN verified: restore the signer from the OS keychain so signed
    // operations work again. Swallowed — the signer lock overlay offers the
    // recovery phrase as a last resort when no keychain entry exists.
    final session = context.read<SessionService>();
    final pubkey = session.activePubkey;
    if (pubkey != null) {
      final signer = context.read<SignerService>();
      try {
        await signer.unlockFromKeyring(pubkey);
      } catch (e) {
        debugPrint('keychain restore: $e');
      }
    }
  }

  void _pressDigit(String d) {
    if (_pin.text.length >= 12) return;
    _pin.text = _pin.text + d;
  }

  @override
  Widget build(BuildContext context) {
    final shell = context.watch<ShellService>();
    return Scaffold(
      backgroundColor: Theme.of(context).colorScheme.surface,
      body: SafeArea(
        child: Center(
          child: SingleChildScrollView(
            padding: const EdgeInsets.all(24),
            child: Column(
              mainAxisAlignment: MainAxisAlignment.center,
              children: [
                Icon(
                  Icons.lock_outline,
                  size: 56,
                  color: Theme.of(context).colorScheme.primary,
                ),
                const SizedBox(height: 16),
                Text(
                  'Soshal is locked',
                  style: Theme.of(context).textTheme.headlineSmall,
                ),
                const SizedBox(height: 24),
                Row(
                  mainAxisAlignment: MainAxisAlignment.center,
                  children: List.generate(
                    6,
                    (i) => Container(
                      width: 14,
                      height: 14,
                      margin: const EdgeInsets.symmetric(horizontal: 4),
                      decoration: BoxDecoration(
                        shape: BoxShape.circle,
                        border: Border.all(
                          color: Theme.of(context).colorScheme.primary,
                          width: 2,
                        ),
                        color: i < _pin.text.length
                            ? Theme.of(context).colorScheme.primary
                            : Colors.transparent,
                      ),
                    ),
                  ),
                ),
                const SizedBox(height: 16),
                if (shell.permanentLocked)
                  Text(
                    'Account permanently locked after too many failed attempts.',
                    style:
                        TextStyle(color: Theme.of(context).colorScheme.error),
                    textAlign: TextAlign.center,
                  ),
                if (shell.lockoutRemaining > 0 && !shell.permanentLocked)
                  Text('Try again in ${shell.lockoutRemaining} seconds'),
                if (shell.unlockError != null &&
                    shell.lockAttempts > 0 &&
                    !shell.permanentLocked)
                  Text(
                    'Wrong PIN — ${shell.lockAttempts} failed attempt(s)',
                    style:
                        TextStyle(color: Theme.of(context).colorScheme.error),
                  ),
                if (shell.unlockError != null &&
                    shell.lockAttempts == 0 &&
                    !shell.permanentLocked)
                  Text(
                    shell.unlockError!,
                    style:
                        TextStyle(color: Theme.of(context).colorScheme.error),
                  ),
                if (shell.lockoutRemaining <= 0 && !shell.permanentLocked) ...[
                  const SizedBox(height: 16),
                  _Keypad(
                    onDigit: _pressDigit,
                    onBackspace: () {
                      if (_pin.text.isNotEmpty) {
                        _pin.text =
                            _pin.text.substring(0, _pin.text.length - 1);
                      }
                    },
                    onClear: () => _pin.clear(),
                    onUnlock: () => _submit(shell),
                  ),
                ],
              ],
            ),
          ),
        ),
      ),
    );
  }
}

class _Keypad extends StatelessWidget {
  final void Function(String) onDigit;
  final VoidCallback onBackspace;
  final VoidCallback onClear;
  final VoidCallback onUnlock;

  const _Keypad({
    required this.onDigit,
    required this.onBackspace,
    required this.onClear,
    required this.onUnlock,
  });

  @override
  Widget build(BuildContext context) {
    final keys = ['1', '2', '3', '4', '5', '6', '7', '8', '9'];
    return Column(
      children: [
        Wrap(
          alignment: WrapAlignment.center,
          spacing: 12,
          runSpacing: 12,
          children: keys.map((k) {
            return SizedBox(
              width: 64,
              height: 56,
              child: OutlinedButton(
                onPressed: () => onDigit(k),
                child: Text(k, style: const TextStyle(fontSize: 18)),
              ),
            );
          }).toList(),
        ),
        const SizedBox(height: 12),
        Row(
          mainAxisAlignment: MainAxisAlignment.center,
          children: [
            SizedBox(
              width: 64,
              child: IconButton(
                  onPressed: onBackspace,
                  icon: const Icon(Icons.backspace_outlined)),
            ),
            const SizedBox(width: 12),
            SizedBox(
              width: 64,
              height: 56,
              child: OutlinedButton(
                onPressed: () => onDigit('0'),
                child: const Text('0', style: TextStyle(fontSize: 18)),
              ),
            ),
            const SizedBox(width: 12),
            SizedBox(
                width: 64,
                child: IconButton(
                    onPressed: onClear, icon: const Icon(Icons.clear))),
          ],
        ),
        const SizedBox(height: 16),
        FilledButton(onPressed: onUnlock, child: const Text('Unlock')),
      ],
    );
  }
}
