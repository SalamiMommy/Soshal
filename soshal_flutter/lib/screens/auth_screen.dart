// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';
import '../services/sync_service.dart';
import '../services/auth_service.dart';
import '../services/error_log.dart';
import '../utils/format.dart';
import '../services/network_service.dart';
import '../services/session_service.dart';
import '../services/shell_service.dart';
import '../services/signer_service.dart';

/// Auth Flow Screen
/// Login with mnemonic/nsec, generate new key
class AuthScreen extends StatefulWidget {
  const AuthScreen({super.key});

  @override
  State<AuthScreen> createState() => _AuthScreenState();
}

class _AuthScreenState extends State<AuthScreen> {
  int _currentStep = 0;
  String? _mnemonic;

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: const Text('Create Account'),
        elevation: 0,
      ),
      body: _buildStep(_currentStep),
    );
  }

  Widget _buildStep(int step) {
    switch (step) {
      case 0:
        return _buildWelcomeStep();
      case 1:
        return _buildGenerateOrImportStep();
      case 2:
        return _buildGenerateMnemonicStep();
      case 3:
        return _buildImportMnemonicStep();
      case 4:
        return _buildConfirmMnemonicStep();
      default:
        return _buildWelcomeStep();
    }
  }

  Widget _buildWelcomeStep() {
    final accounts = context.select((SessionService s) => s.getAccounts());
    final activePubkey = context.select((SessionService s) => s.activePubkey);
    return Center(
      child: SingleChildScrollView(
        padding: const EdgeInsets.all(24),
        child: Column(
          mainAxisAlignment: MainAxisAlignment.center,
          children: [
            const Text(
              'Welcome to Soshal',
              style: TextStyle(fontSize: 28, fontWeight: FontWeight.bold),
            ),
            const SizedBox(height: 16),
            const Text(
              'A decentralized social network',
              style: TextStyle(fontSize: 16, color: Colors.grey),
            ),
            if (accounts.isNotEmpty) ...[
              const SizedBox(height: 32),
              const Text(
                'Use existing account',
                style: TextStyle(fontSize: 18, fontWeight: FontWeight.w600),
              ),
              const SizedBox(height: 8),
              for (final account in accounts)
                Card(
                  child: ListTile(
                    leading: account.pubkey == activePubkey
                        ? const Icon(Icons.check_circle, color: Colors.green)
                        : const Icon(Icons.account_circle),
                    title: Text(
                      account.npub.isNotEmpty
                          ? account.npub
                          : prefixEllipsis(account.pubkey, 16),
                    ),
                    subtitle: Text(prefixEllipsis(account.pubkey, 24)),
                    onTap: () => _useExistingAccount(account),
                  ),
                ),
              const SizedBox(height: 16),
              const Text('or', style: TextStyle(color: Colors.grey)),
            ],
            const SizedBox(height: 16),
            ElevatedButton(
              onPressed: () {
                setState(() => _currentStep = 1);
              },
              style: ElevatedButton.styleFrom(
                padding: const EdgeInsets.symmetric(
                  horizontal: 48,
                  vertical: 16,
                ),
              ),
              child: const Text('Get Started'),
            ),
          ],
        ),
      ),
    );
  }

  /// Recovery-phrase import is the fallback when keychain unlock is disabled
  /// or has no stored key for the account. Route the existing account to the
  /// Unlock a locally stored account and make it active.
  Future<void> _useExistingAccount(SessionAccount account) async {
    final signer = context.read<SignerService>();
    final session = context.read<SessionService>();
    final shell = context.read<ShellService>();
    final pinUser = shell.hasPin;
    try {
      if (!pinUser) {
        final unlocked = await signer.unlockFromKeyring(account.pubkey);
        if (!unlocked) {
          if (mounted) {
            ScaffoldMessenger.of(context).showSnackBar(
              const SnackBar(
                content: SelectableText('Could not unlock local profile.'),
              ),
            );
          }
          return;
        }
      }
      await session.switchAccount(account.pubkey);
      await session.saveSession();
      // Start the Rust-side background relay sync for the newly active
      // account so its feed/DM ingest runs.
      final relays = session.activeAccount?.relayList ?? <String>[];
      if (relays.isNotEmpty && mounted) {
        context.read<SyncService>().start(relays: relays);
      }
      if (mounted) {
        context.go('/feed');
      }
    } catch (e, st) {
      logRuntimeError('existing account unlock: $e', st);
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: SelectableText('Could not unlock account: $e')),
        );
      }
    }
  }

  Widget _buildGenerateOrImportStep() {
    return Center(
      child: Column(
        mainAxisAlignment: MainAxisAlignment.center,
        children: [
          const Text(
            'Create or Import?',
            style: TextStyle(fontSize: 24, fontWeight: FontWeight.bold),
          ),
          const SizedBox(height: 32),
          SizedBox(
            width: double.maxFinite,
            child: Column(
              children: [
                ElevatedButton(
                  onPressed: () {
                    setState(() => _currentStep = 2);
                  },
                  style: ElevatedButton.styleFrom(
                    padding: const EdgeInsets.symmetric(
                      horizontal: 32,
                      vertical: 16,
                    ),
                  ),
                  child: const Text('Generate New Key'),
                ),
                const SizedBox(height: 16),
                OutlinedButton(
                  onPressed: () {
                    setState(() => _currentStep = 3);
                  },
                  style: OutlinedButton.styleFrom(
                    padding: const EdgeInsets.symmetric(
                      horizontal: 32,
                      vertical: 16,
                    ),
                  ),
                  child: const Text('Import from friends\' cache'),
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }

  Widget _buildGenerateMnemonicStep() {
    return GenerateMnemonicWidget(
      onNext: (mnemonic) {
        _mnemonic = mnemonic;
        setState(() => _currentStep = 4);
      },
      onBack: () {
        setState(() => _currentStep = 1);
      },
    );
  }

  Widget _buildImportMnemonicStep() {
    return ImportMnemonicWidget(
      onNext: (mnemonic) {
        _mnemonic = mnemonic;
        setState(() => _currentStep = 4);
      },
      onBack: () {
        setState(() => _currentStep = 1);
      },
    );
  }

  Widget _buildConfirmMnemonicStep() {
    final mnemonic = _mnemonic;
    if (mnemonic == null) {
      return const Text('No recovery phrase available');
    }
    return ConfirmMnemonicWidget(
      mnemonic: mnemonic,
      onComplete: () {
        setState(() => _mnemonic = null);
        context.go('/feed');
      },
      onBack: () {
        setState(() => _currentStep = 2);
      },
    );
  }
}

/// Generate Mnemonic Widget
class GenerateMnemonicWidget extends StatefulWidget {
  final ValueChanged<String> onNext;
  final VoidCallback onBack;

  const GenerateMnemonicWidget({
    super.key,
    required this.onNext,
    required this.onBack,
  });

  @override
  State<GenerateMnemonicWidget> createState() => _GenerateMnemonicWidgetState();
}

class _GenerateMnemonicWidgetState extends State<GenerateMnemonicWidget> {
  String? _mnemonic;
  bool _isLoading = false;

  @override
  void initState() {
    super.initState();
    _generateMnemonic();
  }

  Future<void> _generateMnemonic() async {
    setState(() => _isLoading = true);
    try {
      final authService = context.read<AuthService>();
      final mnemonic = await authService.generateMnemonic();
      if (mounted) setState(() => _mnemonic = mnemonic);
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: SelectableText('Error: $e')),
        );
      }
    } finally {
      if (mounted) setState(() => _isLoading = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.all(16),
      child: Column(
        mainAxisAlignment: MainAxisAlignment.center,
        children: [
          const Text(
            'Your Recovery Phrase',
            style: TextStyle(fontSize: 24, fontWeight: FontWeight.bold),
          ),
          const SizedBox(height: 16),
          const Text(
            'Save this phrase in a safe place. Never share it with anyone.',
            style: TextStyle(fontSize: 14, color: Colors.grey),
          ),
          const SizedBox(height: 32),
          if (_isLoading)
            const CircularProgressIndicator()
          else if (_mnemonic != null)
            Container(
              padding: const EdgeInsets.all(16),
              decoration: BoxDecoration(
                border: Border.all(color: Colors.grey),
                borderRadius: BorderRadius.circular(8),
              ),
              child: SelectableText(
                _mnemonic!,
                style: const TextStyle(fontSize: 16),
              ),
            ),
          const SizedBox(height: 32),
          Row(
            mainAxisAlignment: MainAxisAlignment.spaceEvenly,
            children: [
              OutlinedButton(
                onPressed: widget.onBack,
                child: const Text('Back'),
              ),
              ElevatedButton(
                onPressed: _isLoading || _mnemonic == null
                    ? null
                    : () {
                        final m = _mnemonic!;
                        setState(() => _mnemonic = null);
                        widget.onNext(m);
                      },
                child: const Text('Continue'),
              ),
            ],
          ),
        ],
      ),
    );
  }
}

/// Import Mnemonic Widget
class ImportMnemonicWidget extends StatefulWidget {
  final ValueChanged<String> onNext;
  final VoidCallback onBack;

  const ImportMnemonicWidget({
    super.key,
    required this.onNext,
    required this.onBack,
  });

  @override
  State<ImportMnemonicWidget> createState() => _ImportMnemonicWidgetState();
}

class _ImportMnemonicWidgetState extends State<ImportMnemonicWidget> {
  final _mnemonicController = TextEditingController();
  bool _isLoading = false;

  @override
  void dispose() {
    _mnemonicController.dispose();
    super.dispose();
  }

  Future<void> _validateAndImport() async {
    setState(() => _isLoading = true);
    try {
      final authService = context.read<AuthService>();
      final isValid =
          await authService.validateMnemonic(_mnemonicController.text);
      if (isValid) {
        final text = _mnemonicController.text.trim();
        _mnemonicController.clear();
        widget.onNext(text);
      } else {
        if (mounted) {
          ScaffoldMessenger.of(context).showSnackBar(
            const SnackBar(content: SelectableText('Invalid mnemonic phrase')),
          );
        }
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: SelectableText('Error: $e')),
        );
      }
    } finally {
      if (mounted) setState(() => _isLoading = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.all(16),
      child: Column(
        mainAxisAlignment: MainAxisAlignment.center,
        children: [
          const Text(
            'Import from Friends\' Cache',
            style: TextStyle(fontSize: 24, fontWeight: FontWeight.bold),
          ),
          const SizedBox(height: 16),
          const Text(
            'Enter your recovery phrase to restore your account from your friends\' cache.',
            textAlign: TextAlign.center,
            style: TextStyle(fontSize: 14, color: Colors.grey),
          ),
          const SizedBox(height: 16),
          TextField(
            controller: _mnemonicController,
            decoration: InputDecoration(
              hintText: 'word1 word2 word3 ...',
              border: OutlineInputBorder(
                borderRadius: BorderRadius.circular(8),
              ),
            ),
            maxLines: 3,
          ),
          const SizedBox(height: 32),
          Row(
            mainAxisAlignment: MainAxisAlignment.spaceEvenly,
            children: [
              OutlinedButton(
                onPressed: _isLoading ? null : widget.onBack,
                child: const Text('Back'),
              ),
              ElevatedButton(
                onPressed: _isLoading ? null : _validateAndImport,
                child: const Text('Continue'),
              ),
            ],
          ),
        ],
      ),
    );
  }
}

/// Confirm Mnemonic Widget
class ConfirmMnemonicWidget extends StatefulWidget {
  final String mnemonic;
  final VoidCallback onComplete;
  final VoidCallback onBack;

  const ConfirmMnemonicWidget({
    super.key,
    required this.mnemonic,
    required this.onComplete,
    required this.onBack,
  });

  @override
  State<ConfirmMnemonicWidget> createState() => _ConfirmMnemonicWidgetState();
}

class _ConfirmMnemonicWidgetState extends State<ConfirmMnemonicWidget> {
  bool _isLoading = false;

  Future<void> _complete() async {
    setState(() => _isLoading = true);
    try {
      final authService = context.read<AuthService>();
      final sessionService = context.read<SessionService>();
      final signer = context.read<SignerService>();

      // Restore keypair from the confirmed recovery phrase
      final keypair = await authService.restoreFromMnemonic(
        widget.mnemonic.trim(),
        '',
      );
      final npub = await authService.encodeNpub(keypair.publicKey);

      // Load/refresh session state (both Dart side and the Rust SESSION
      // static) before mutating — do not rely on splash having run it.
      await sessionService.loadSession();

      // Add to session
      const defaultRelays = NetworkService.defaultRelays;
      if (!sessionService
          .getAccounts()
          .any((a) => a.pubkey == keypair.publicKey)) {
        await sessionService.addAccount(
          keypair.publicKey,
          npub,
          defaultRelays,
        );
      }

      if (await signer.isLocked()) {
        try {
          await authService.restoreFromMnemonic(
            widget.mnemonic.trim(),
            '',
          );
        } catch (e) {
          if (mounted) {
            setState(() => _isLoading = false);
            ScaffoldMessenger.of(context).showSnackBar(
              SnackBar(
                content: const SelectableText(
                  'Session locked during setup — tap Go to Feed to retry.',
                ),
                action: SnackBarAction(
                  label: 'Retry',
                  onPressed: _complete,
                ),
              ),
            );
          }
          return;
        }
      }

      // Make the just-created/imported identity active so the next launch
      // auto-logs into it (addAccount only activates when no account is
      // active yet — otherwise the first account ever stays active forever).
      await sessionService.switchAccount(keypair.publicKey);

      // Seed a base profile row so the user is indexable/searchable
      try {
        await sessionService.storeProfile(jsonEncode({
          'pubkey': keypair.publicKey,
          'content': {
            'name': '',
            'display_name': 'New user',
            'about': '',
          },
        }));
      } catch (e, st) {
        debugPrint('onboarding profile save failed: $e');
        logRuntimeError('onboarding profile save: $e', st);
      }

      // Save session
      await sessionService.saveSession();

      // The session switch invalidates signer keys for any previously
      // unlocked identity (a different account's keys must never linger in
      // memory). Re-unlock the just-restored identity with the recovery
      // phrase in hand before the keychain save below — saving requires the
      // in-process signer for THIS pubkey to be unlocked.
      if (await signer.isLocked()) {
        await authService.restoreFromMnemonic(
          widget.mnemonic.trim(),
          '',
        );
      }

      // Persist the nsec to the OS keychain so future launches can restore
      // the signer without the recovery phrase (desktop keychains only;
      // swallowed when unavailable, e.g. Android without keystore backend).
      try {
        final fullySaved = await signer.saveToKeyring(keypair.publicKey);
        if (!fullySaved && mounted) {
          // OS keychain unavailable — local sealed storage succeeded.
          // Warn the user: the next launch may still require the recovery
          // phrase if the sealed file is lost (device wipe, app uninstall).
          ScaffoldMessenger.of(context).showSnackBar(
            const SnackBar(
              content: SelectableText(
                'Key saved locally — OS keychain unavailable. '
                'Biometric protection is not active. '
                'The next launch may require your recovery phrase.',
              ),
            ),
          );
        }
      } catch (e, st) {
        debugPrint('onboarding keychain save failed: $e');
        logRuntimeError('onboarding keychain save: $e', st);
        if (mounted) {
          ScaffoldMessenger.of(context).showSnackBar(
            const SnackBar(
              content: SelectableText(
                'Could not remember this device — the next launch will '
                'require your recovery phrase.',
              ),
            ),
          );
        }
      }

      // Refresh signer state to reflect that it's now unlocked
      await signer.refresh();

      // Start the Rust-side background relay sync for the new account.
      if (mounted) {
        context.read<SyncService>().start(relays: defaultRelays);
      }

      if (mounted) {
        widget.onComplete();
      }
    } catch (e, st) {
      debugPrint('auth_complete failed: $e');
      // Stack trace not printed in release: it may contain file paths / flow
      // details near mnemonic handling. Redacted error already surfaced below.
      assert(() {
        debugPrint('$st');
        return true;
      }());
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: SelectableText('Error: $e')),
        );
      }
    } finally {
      if (mounted) setState(() => _isLoading = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.all(16),
      child: Column(
        mainAxisAlignment: MainAxisAlignment.center,
        children: [
          const Text(
            'Account Created',
            style: TextStyle(fontSize: 24, fontWeight: FontWeight.bold),
          ),
          const SizedBox(height: 16),
          const Text(
            'Your account is ready!',
            style: TextStyle(fontSize: 14, color: Colors.grey),
          ),
          const SizedBox(height: 48),
          if (_isLoading)
            const CircularProgressIndicator()
          else
            ElevatedButton(
              onPressed: _complete,
              style: ElevatedButton.styleFrom(
                padding: const EdgeInsets.symmetric(
                  horizontal: 48,
                  vertical: 16,
                ),
              ),
              child: const Text('Go to Feed'),
            ),
        ],
      ),
    );
  }
}
