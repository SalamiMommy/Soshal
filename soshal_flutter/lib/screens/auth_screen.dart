// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';
import '../services/sync_service.dart';
import '../services/auth_service.dart';
import '../services/error_log.dart';
import '../services/session_service.dart';

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
    return Center(
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
          const SizedBox(height: 48),
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
    );
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
                  child: const Text('Import Existing Key'),
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
        // Save session and route to feed
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
      setState(() => _mnemonic = mnemonic);
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: SelectableText('Error: $e')),
        );
      }
    } finally {
      setState(() => _isLoading = false);
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
                    : () => widget.onNext(_mnemonic!),
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
        widget.onNext(_mnemonicController.text.trim());
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
      setState(() => _isLoading = false);
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
            'Enter Your Recovery Phrase',
            style: TextStyle(fontSize: 24, fontWeight: FontWeight.bold),
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
      const defaultRelays = ['wss://relay.nostr.band', 'wss://nos.lol'];
      await sessionService.addAccount(
        keypair.publicKey,
        npub,
        defaultRelays,
      );

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

      // Start the Rust-side background relay sync for the new account.
      if (mounted) {
        context.read<SyncService>().start(relays: defaultRelays);
      }

      if (mounted) {
        widget.onComplete();
      }
    } catch (e, st) {
      debugPrint('auth_complete failed: $e\n$st');
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: SelectableText('Error: $e')),
        );
      }
    } finally {
      setState(() => _isLoading = false);
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
