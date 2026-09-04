import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import '../services/auth_service.dart';
import '../services/session_service.dart';
import '../services/settings_service.dart';
import '../services/shell_service.dart';
import '../services/signer_service.dart';
import '../utils/format.dart';
import '../widgets/settings_scaffold.dart';
import '../widgets/error_state_text.dart';

/// Security settings: honest status of protections on this build.
class SecurityScreen extends StatefulWidget {
  /// Security settings screen.
  const SecurityScreen({super.key});

  @override
  State<SecurityScreen> createState() => _SecurityScreenState();
}

class _SecurityScreenState extends State<SecurityScreen> {
  String? _pubkey;
  bool? _locked;
  bool _keychainUnlockEnabled = false;
  bool _autologinEnabled = true;

  @override
  void initState() {
    super.initState();
    _loadSettings();
    _refresh();
  }

  void _loadSettings() {
    final settings = context.read<SettingsService>();
    final value = settings.getSetting('keychain_unlock_enabled');
    setState(() {
      _keychainUnlockEnabled = value == 'true';
      _autologinEnabled = settings.getSetting('autologin_enabled') != 'false';
    });
  }

  Future<void> _refresh() async {
    final signer = context.read<SignerService>();
    String? pubkey;
    bool? locked;
    try {
      pubkey = await signer.pubkey();
      locked = await signer.isLocked();
    } catch (e) { debugPrint('security status fetch: $e'); }
    if (mounted) {
      setState(() {
        _pubkey = pubkey;
        _locked = locked;
      });
    }
  }

  Future<void> _confirmAndLock() async {
    final signer = context.read<SignerService>();
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
    final signer = context.read<SignerService>();
    return SettingsScaffold(
      title: 'Security',
      children: [
        const ListTile(
          leading: Icon(Icons.lock_outline),
          title: Text('Key storage'),
          subtitle:
              Text('Secret keys live in the OS keychain (Keystore on Android). '
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
            _pubkey == null ? '…' : prefixEllipsis(_pubkey!, 16),
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
              try {
                await signer.saveToKeyring(pubkey);
                if (context.mounted) {
                  ScaffoldMessenger.of(context).showSnackBar(
                    SnackBar(
                        content: SelectableText('Saved to keychain ($pubkey)')),
                  );
                }
              } catch (e) {
                if (context.mounted) {
                  ScaffoldMessenger.of(context).showSnackBar(
                    SnackBar(content: SelectableText('Keychain error: $e')),
                  );
                }
              }
            },
            child: const Text('Save'),
          ),
        ),
        SwitchListTile(
          dense: true,
          title: const Text('Enable keychain unlock'),
          subtitle: const Text('Allow unlocking from device keychain'),
          value: _keychainUnlockEnabled,
          onChanged: (value) async {
            final settings = context.read<SettingsService>();
            await settings.setSetting(
              'keychain_unlock_enabled',
              value ? 'true' : 'false',
            );
            setState(() => _keychainUnlockEnabled = value);
          },
        ),
        SwitchListTile(
          dense: true,
          title: const Text('Autologin'),
          subtitle: const Text(
              'Sign in to the last account used automatically on launch'),
          value: _autologinEnabled,
          onChanged: (value) async {
            final settings = context.read<SettingsService>();
            await settings.setSetting('autologin_enabled', value.toString());
            setState(() {
              _autologinEnabled = value;
            });
          },
        ),
        ListTile(
          dense: true,
          title: const Text('Unlock from device keychain'),
          enabled: _keychainUnlockEnabled,
          trailing: OutlinedButton(
            onPressed: _keychainUnlockEnabled
                ? () async {
                    if (pubkey.isEmpty) return;
                    try {
                      await signer.unlockFromKeyring(pubkey);
                      await _refresh();
                      if (context.mounted) {
                        ScaffoldMessenger.of(context).showSnackBar(
                          const SnackBar(
                              content:
                                  SelectableText('Unlocked from keychain')),
                        );
                      }
                    } catch (e) {
                      if (context.mounted) {
                        ScaffoldMessenger.of(context).showSnackBar(
                          SnackBar(
                              content:
                                  SelectableText('Keychain unlock error: $e')),
                        );
                      }
                    }
                  }
                : null,
            child: const Text('Unlock'),
          ),
        ),
        ListTile(
          dense: true,
          title: const Text('Remove key from device keychain'),
          trailing: OutlinedButton(
            onPressed: () async {
              if (pubkey.isEmpty) return;
              try {
                await signer.removeFromKeyring(pubkey);
                if (context.mounted) {
                  ScaffoldMessenger.of(context).showSnackBar(
                    const SnackBar(
                        content: SelectableText('Removed from keychain')),
                  );
                }
              } catch (e) {
                if (context.mounted) {
                  ScaffoldMessenger.of(context).showSnackBar(
                    SnackBar(
                        content: SelectableText('Keychain remove error: $e')),
                  );
                }
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
        const Divider(),
        Padding(
          padding: const EdgeInsets.only(left: 16, top: 8),
          child: Text('Key tools',
              style:
                  const TextStyle(fontSize: 16, fontWeight: FontWeight.bold)),
        ),
        ListTile(
          dense: true,
          leading: const Icon(Icons.vpn_key_outlined),
          title: const Text('Generate new keypair'),
          subtitle: const Text('Create a fresh identity (pubkey + nsec)'),
          trailing: OutlinedButton(
            onPressed: _generateKeypairDialog,
            child: const Text('Generate'),
          ),
        ),
        ListTile(
          dense: true,
          leading: const Icon(Icons.key_outlined),
          title: const Text('Derive pubkey from nsec'),
          subtitle: const Text('Recover the npub for a secret key — '
              'no import, no key storage'),
          trailing: OutlinedButton(
            onPressed: _derivePubkeyDialog,
            child: const Text('Derive'),
          ),
        ),
        ListTile(
          dense: true,
          leading: const Icon(Icons.lock_clock_outlined),
          title: const Text('Lock app (PIN)'),
          subtitle: const Text('Show the app lock gate until the PIN is '
              'entered again'),
          trailing: OutlinedButton(
            onPressed: () {
              final shell = context.read<ShellService>();
              if (!shell.hasPin && !shell.biometricsEnabled) {
                ScaffoldMessenger.of(context).showSnackBar(
                  const SnackBar(
                      content: SelectableText(
                          'No app lock PIN configured — set one in '
                          'Settings → Privacy')),
                );
                return;
              }
              shell.lockNow();
              ScaffoldMessenger.of(context).showSnackBar(
                const SnackBar(content: SelectableText('App locked')),
              );
            },
            child: const Text('Lock now'),
          ),
        ),
        const Divider(),
        Padding(
          padding: const EdgeInsets.only(left: 16, top: 8),
          child: Text('Crypto tools',
              style:
                  const TextStyle(fontSize: 16, fontWeight: FontWeight.bold)),
        ),
        ListTile(
          dense: true,
          leading: const Icon(Icons.draw_outlined),
          title: const Text('Schnorr sign digest'),
          subtitle: const Text('SHA-256 the input Dart-side, Schnorr-sign '
              'the digest with the unlocked key'),
          trailing: OutlinedButton(
            onPressed: _schnorrSignDialog,
            child: const Text('Sign'),
          ),
        ),
        ListTile(
          dense: true,
          leading: const Icon(Icons.event_note_outlined),
          title: const Text('Sign event JSON'),
          subtitle: const Text('Sign an unsigned NIP-59-style event '
              '(pubkey, created_at, kind, tags, content)'),
          trailing: OutlinedButton(
            onPressed: _signUnsignedDialog,
            child: const Text('Sign'),
          ),
        ),
        ListTile(
          dense: true,
          leading: const Icon(Icons.lock_outline),
          title: const Text('NIP-44 encrypt / decrypt'),
          subtitle: const Text('Round-trip a message to a recipient '
              'pubkey — encrypt then decrypt back'),
          trailing: OutlinedButton(
            onPressed: _nip44Dialog,
            child: const Text('Run'),
          ),
        ),
      ],
    );
  }

  Future<void> _schnorrSignDialog() async {
    final signer = context.read<SignerService>();
    final controller = TextEditingController();
    String? signature;
    String? error;
    await showDialog<void>(
      context: context,
      builder: (context) => StatefulBuilder(
        builder: (context, setDialogState) {
          Future<void> doSign() async {
            try {
              final sig = await signer.schnorrSign(controller.text.trim());
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
            title: const Text('Schnorr sign digest'),
            content: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                TextField(
                  controller: controller,
                  autofocus: true,
                  maxLines: 3,
                  decoration: const InputDecoration(
                    hintText: 'Message to hash + sign',
                    border: OutlineInputBorder(),
                  ),
                ),
                const SizedBox(height: 12),
                if (signature != null)
                  SelectableText('Signature:\n$signature',
                      style: const TextStyle(
                          fontSize: 11, fontFamily: 'monospace')),
                if (error != null) ErrorStateText('Error: $error'),
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

  Future<void> _signUnsignedDialog() async {
    final signer = context.read<SignerService>();
    final controller = TextEditingController();
    String? signedJson;
    String? error;
    await showDialog<void>(
      context: context,
      builder: (context) => StatefulBuilder(
        builder: (context, setDialogState) {
          Future<void> doSign() async {
            try {
              final signed = await signer.signUnsigned(controller.text.trim());
              setDialogState(() {
                signedJson = signed;
                error = null;
              });
            } catch (e) {
              setDialogState(() {
                error = e.toString();
                signedJson = null;
              });
            }
          }

          return AlertDialog(
            title: const Text('Sign event JSON'),
            content: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                TextField(
                  controller: controller,
                  autofocus: true,
                  maxLines: 6,
                  decoration: const InputDecoration(
                    hintText: '{"pubkey":"…","created_at":…,"kind":…,'
                        '"tags":[],"content":"…"}',
                    border: OutlineInputBorder(),
                  ),
                ),
                const SizedBox(height: 12),
                if (signedJson != null)
                  SelectableText(signedJson!,
                      style: const TextStyle(
                          fontSize: 11, fontFamily: 'monospace')),
                if (error != null) ErrorStateText('Error: $error'),
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

  Future<void> _nip44Dialog() async {
    final signer = context.read<SignerService>();
    final textController = TextEditingController();
    final pubkeyController = TextEditingController(text: _pubkey ?? '');
    String? ciphertext;
    String? decrypted;
    String? error;
    await showDialog<void>(
      context: context,
      builder: (context) => StatefulBuilder(
        builder: (context, setDialogState) {
          Future<void> doRoundTrip() async {
            try {
              final ct = await signer.nip44Encrypt(
                  textController.text.trim(), pubkeyController.text.trim());
              final pt =
                  await signer.nip44Decrypt(ct, pubkeyController.text.trim());
              setDialogState(() {
                ciphertext = ct;
                decrypted = pt;
                error = null;
              });
            } catch (e) {
              setDialogState(() {
                error = e.toString();
                ciphertext = null;
                decrypted = null;
              });
            }
          }

          return AlertDialog(
            title: const Text('NIP-44 round-trip'),
            content: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                TextField(
                  controller: textController,
                  autofocus: true,
                  maxLines: 2,
                  decoration: const InputDecoration(
                    hintText: 'Plaintext to encrypt',
                    border: OutlineInputBorder(),
                  ),
                ),
                const SizedBox(height: 8),
                TextField(
                  controller: pubkeyController,
                  decoration: const InputDecoration(
                    labelText: 'Recipient pubkey (hex)',
                    border: OutlineInputBorder(),
                    isDense: true,
                  ),
                ),
                const SizedBox(height: 12),
                if (ciphertext != null) ...[
                  const Text('Ciphertext',
                      style: TextStyle(fontSize: 11, color: Colors.grey)),
                  SelectableText(ciphertext!,
                      style: const TextStyle(
                          fontSize: 11, fontFamily: 'monospace')),
                  const SizedBox(height: 8),
                  const Text('Decrypted',
                      style: TextStyle(fontSize: 11, color: Colors.grey)),
                  SelectableText(decrypted ?? '',
                      style: const TextStyle(
                          fontSize: 11, fontFamily: 'monospace')),
                  if (decrypted != null)
                    Text(
                      decrypted == textController.text.trim()
                          ? 'Round-trip match ✓'
                          : 'Round-trip mismatch',
                      style: TextStyle(
                        fontSize: 11,
                        color: decrypted == textController.text.trim()
                            ? Colors.green
                            : Colors.orange,
                      ),
                    ),
                ],
                if (error != null) ErrorStateText('Error: $error'),
              ],
            ),
            actions: [
              TextButton(
                onPressed: () => Navigator.pop(context),
                child: const Text('Close'),
              ),
              FilledButton(
                onPressed: () {
                  if (textController.text.trim().isEmpty ||
                      pubkeyController.text.trim().isEmpty) {
                    return;
                  }
                  doRoundTrip();
                },
                child: const Text('Encrypt + decrypt'),
              ),
            ],
          );
        },
      ),
    );
  }

  Future<void> _signDialog() async {
    final signer = context.read<SignerService>();
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
                if (error != null) ErrorStateText('Error: $error'),
                const SizedBox(height: 8),
                Text(
                  'Public key: ${prefixEllipsis(_pubkey ?? '', 12)}',
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

  Future<void> _generateKeypairDialog() async {
    final auth = context.read<AuthService>();
    String? pubkey;
    String? mnemonic;
    String? error;
    await showDialog<void>(
      context: context,
      builder: (context) => StatefulBuilder(
        builder: (context, setDialogState) {
          Future<void> doGenerate() async {
            try {
              final kp = await auth.generateKeypair();
              final m = await auth.generateMnemonic();
              setDialogState(() {
                pubkey = kp.publicKey;
                mnemonic = m;
                error = null;
              });
            } catch (e) {
              setDialogState(() {
                error = e.toString();
                pubkey = null;
                mnemonic = null;
              });
            }
          }

          return AlertDialog(
            title: const Text('Generate new keypair'),
            content: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                if (pubkey != null && mnemonic != null) ...[
                  const Text('Public key (hex)',
                      style: TextStyle(fontSize: 11, color: Colors.grey)),
                  SelectableText(pubkey!,
                      style: const TextStyle(
                          fontSize: 11, fontFamily: 'monospace')),
                  const SizedBox(height: 8),
                  const Text('Backup phrase (BIP-39)',
                      style: TextStyle(fontSize: 11, color: Colors.grey)),
                  SelectableText(mnemonic!,
                      style: const TextStyle(
                          fontSize: 11, fontFamily: 'monospace')),
                  const SizedBox(height: 8),
                  const Text(
                    'The backup phrase is shown only once — copy it now. '
                    'You can restore your identity from it at any time. '
                    'The secret key itself never leaves the app.',
                    style: TextStyle(fontSize: 12, color: Colors.orange),
                  ),
                ] else
                  const Text('A fresh Nostr identity will be generated.'),
                if (error != null) ErrorStateText('Error: $error'),
              ],
            ),
            actions: [
              TextButton(
                onPressed: () => Navigator.pop(context),
                child: const Text('Close'),
              ),
              FilledButton(
                onPressed: pubkey == null ? doGenerate : null,
                child: Text(pubkey == null ? 'Generate' : 'Done'),
              ),
            ],
          );
        },
      ),
    );
    // Secret shown once in the dialog; drop it from the service heap now.
    auth.clearSecretKey();
  }

  Future<void> _derivePubkeyDialog() async {
    final auth = context.read<AuthService>();
    final controller = TextEditingController();
    String? npub;
    String? error;
    await showDialog<void>(
      context: context,
      builder: (context) => StatefulBuilder(
        builder: (context, setDialogState) {
          Future<void> doDerive() async {
            try {
              final derived =
                  await auth.getPublicKeyFromNsec(controller.text.trim());
              setDialogState(() {
                npub = derived;
                error = null;
              });
            } catch (e) {
              setDialogState(() {
                error = e.toString();
                npub = null;
              });
            }
          }

          return AlertDialog(
            title: const Text('Derive pubkey from nsec'),
            content: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                TextField(
                  controller: controller,
                  autofocus: true,
                  obscureText: true,
                  decoration: const InputDecoration(
                    hintText: 'nsec1…',
                    border: OutlineInputBorder(),
                  ),
                ),
                const SizedBox(height: 12),
                if (npub != null) ...[
                  const Text('Public key (npub)',
                      style: TextStyle(fontSize: 11, color: Colors.grey)),
                  SelectableText(npub!,
                      style: const TextStyle(
                          fontSize: 11, fontFamily: 'monospace')),
                ],
                if (error != null) ErrorStateText('Error: $error'),
              ],
            ),
            actions: [
              TextButton(
                onPressed: () => Navigator.pop(context),
                child: const Text('Close'),
              ),
              FilledButton(
                onPressed: controller.text.trim().isEmpty ? null : doDerive,
                child: const Text('Derive'),
              ),
            ],
          );
        },
      ),
    );
  }
}
