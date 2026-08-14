import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import '../services/session_service.dart';
import '../services/signer_service.dart';

/// Security settings: honest status of protections on this build.
class SecurityScreen extends StatefulWidget {
  /// Security settings screen.
  const SecurityScreen({super.key});

  @override
  State<SecurityScreen> createState() => _SecurityScreenState();
}

class _SecurityScreenState extends State<SecurityScreen> {
  final signer = SignerService();
  String? _pubkey;
  bool? _locked;

  @override
  void initState() {
    super.initState();
    _refresh();
  }

  Future<void> _refresh() async {
    String? pubkey;
    bool? locked;
    try {
      pubkey = await signer.pubkey();
      locked = await signer.isLocked();
    } catch (_) {}
    if (mounted) {
      setState(() {
        _pubkey = pubkey;
        _locked = locked;
      });
    }
  }

  Future<void> _confirmAndLock() async {
    final doIt = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Lock session?'),
        content: const Text(
            'In-memory keys will be wiped. Unlock with your keychain '
            'entry or recovery phrase.'),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: const Text('Cancel'),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, true),
            child: const Text('Lock'),
          ),
        ],
      ),
    );
    if (doIt != true) return;
    await signer.lock();
    await _refresh();
  }

  @override
  Widget build(BuildContext context) {
    final pubkey = context.read<SessionService>().activePubkey ?? '';
    return Scaffold(
      appBar: AppBar(title: const Text('Security')),
      body: ListView(
        children: [
          const ListTile(
            leading: Icon(Icons.lock_outline),
            title: Text('Key storage'),
            subtitle: Text(
                'Secret keys live in the OS keychain (Keystore on Android). '
                'Unlocked for the current session only.'),
          ),
          const ListTile(
            leading: Icon(Icons.screenshot_monitor),
            title: Text('Screen capture'),
            subtitle: Text(
                'Blocked on Android by FLAG_SECURE while the app is active.'),
          ),
          const ListTile(
            leading: Icon(Icons.pin_outlined),
            title: Text('App lock PIN'),
            subtitle:
                Text('Not available in this build. Account switch is its own '
                    'gate: switching accounts requires unlocking via recovery '
                    'phrase.'),
          ),
          const ListTile(
            leading: Icon(Icons.shield_outlined),
            title: Text('Encrypted DMs'),
            subtitle:
                Text('Messages use NIP-44 v2 (ChaCha20 + HMAC). Locked bubbles '
                    'must be tapped to decrypt in this build.'),
          ),
          const Divider(),
          Padding(
            padding: const EdgeInsets.only(left: 16, top: 8),
            child: Text('Signer keys',
                style:
                    const TextStyle(fontSize: 16, fontWeight: FontWeight.bold)),
          ),
          ListTile(
            dense: true,
            title: const Text('Signer pubkey'),
            subtitle: Text(
              _pubkey == null
                  ? '…'
                  : (_pubkey!.length > 20
                      ? '${_pubkey!.substring(0, 16)}…'
                      : _pubkey!),
              style: const TextStyle(fontSize: 11),
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
            ),
          ),
          ListTile(
            dense: true,
            title: const Text('Key state'),
            subtitle: Text(
              _locked == true ? 'Locked (keys wiped)' : 'Unlocked',
              style: TextStyle(
                color: _locked == true ? Colors.orange : Colors.green,
              ),
            ),
          ),
          ListTile(
            dense: true,
            title: const Text('Lock now'),
            subtitle: const Text('Zeroize in-memory keys'),
            trailing: OutlinedButton(
              onPressed: _confirmAndLock,
              child: const Text('Lock'),
            ),
          ),
          ListTile(
            dense: true,
            title: const Text('Save key to device keychain'),
            trailing: OutlinedButton(
              onPressed: () async {
                if (pubkey.isEmpty) return;
                await signer.saveToKeyring(pubkey);
                if (context.mounted) {
                  ScaffoldMessenger.of(context).showSnackBar(
                    SnackBar(
                        content: SelectableText('Saved to keychain ($pubkey)')),
                  );
                }
              },
              child: const Text('Save'),
            ),
          ),
          ListTile(
            dense: true,
            title: const Text('Unlock from device keychain'),
            trailing: OutlinedButton(
              onPressed: () async {
                if (pubkey.isEmpty) return;
                await signer.unlockFromKeyring(pubkey);
                await _refresh();
                if (context.mounted) {
                  ScaffoldMessenger.of(context).showSnackBar(
                    const SnackBar(
                        content: SelectableText('Unlocked from keychain')),
                  );
                }
              },
              child: const Text('Unlock'),
            ),
          ),
          ListTile(
            dense: true,
            title: const Text('Remove key from device keychain'),
            trailing: OutlinedButton(
              onPressed: () async {
                if (pubkey.isEmpty) return;
                await signer.removeFromKeyring(pubkey);
                if (context.mounted) {
                  ScaffoldMessenger.of(context).showSnackBar(
                    const SnackBar(
                        content: SelectableText('Removed from keychain')),
                  );
                }
              },
              child: const Text('Remove'),
            ),
          ),
          const Divider(),
          ListTile(
            leading: const Icon(Icons.draw_outlined),
            title: const Text('Sign message'),
            subtitle: const Text('Schnorr-sign the SHA-256 digest of a text — '
                'proof of key ownership, no secret exposed'),
            trailing: const Icon(Icons.chevron_right),
            onTap: _signDialog,
          ),
        ],
      ),
    );
  }

  Future<void> _signDialog() async {
    final controller = TextEditingController();
    String? signature;
    String? error;
    await showDialog<void>(
      context: context,
      builder: (context) => StatefulBuilder(
        builder: (context, setDialogState) {
          Future<void> doSign() async {
            try {
              final sig = await signer.signText(controller.text.trim());
              setDialogState(() {
                signature = sig;
                error = null;
              });
            } catch (e) {
              setDialogState(() {
                error = e.toString();
                signature = null;
              });
            }
          }

          return AlertDialog(
            title: const Text('Sign message'),
            content: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                TextField(
                  controller: controller,
                  autofocus: true,
                  maxLines: 3,
                  decoration: const InputDecoration(
                    hintText: 'Message to sign',
                    border: OutlineInputBorder(),
                  ),
                ),
                const SizedBox(height: 12),
                if (signature != null)
                  SelectableText('Signature:\n$signature',
                      style: const TextStyle(
                          fontSize: 11, fontFamily: 'monospace')),
                if (error != null)
                  Text('Error: $error',
                      style: TextStyle(
                          fontSize: 11,
                          color: Theme.of(context).colorScheme.error)),
                const SizedBox(height: 8),
                Text(
                  'Public key: ${(_pubkey ?? '').substring(0, 12)}…',
                  style: const TextStyle(fontSize: 11, color: Colors.grey),
                ),
              ],
            ),
            actions: [
              TextButton(
                onPressed: () => Navigator.pop(context),
                child: const Text('Close'),
              ),
              FilledButton(
                onPressed: () {
                  if (controller.text.trim().isEmpty) return;
                  doSign();
                },
                child: const Text('Sign'),
              ),
            ],
          );
        },
      ),
    );
  }
}
