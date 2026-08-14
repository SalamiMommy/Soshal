import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:provider/provider.dart';
import '../services/session_service.dart';

/// Share Soshal — invite link for the active account.
class ShareAppScreen extends StatelessWidget {
  const ShareAppScreen({super.key});

  String _inviteUrl(SessionAccount? account) {
    if (account == null || account.npub.isEmpty) return '';
    return 'https://soshal.app/u/${account.npub}';
  }

  @override
  Widget build(BuildContext context) {
    final account = context.watch<SessionService>().activeAccount;
    final url = _inviteUrl(account);

    return Scaffold(
      appBar: AppBar(title: const Text('Share Soshal')),
      body: Center(
        child: Padding(
          padding: const EdgeInsets.all(24),
          child: url.isEmpty
              ? const Column(
                  mainAxisAlignment: MainAxisAlignment.center,
                  children: [
                    Icon(Icons.person_off_outlined, size: 48),
                    SizedBox(height: 12),
                    Text('No active account'),
                  ],
                )
              : Column(
                  mainAxisAlignment: MainAxisAlignment.center,
                  children: [
                    SelectableText(
                      url,
                      textAlign: TextAlign.center,
                      style: Theme.of(context).textTheme.bodyLarge,
                    ),
                    const SizedBox(height: 24),
                    FilledButton.icon(
                      icon: const Icon(Icons.copy),
                      label: const Text('📋 Copy Link'),
                      onPressed: () async {
                        await Clipboard.setData(ClipboardData(text: url));
                        if (!context.mounted) return;
                        ScaffoldMessenger.of(context).showSnackBar(
                          const SnackBar(
                              content: Text('Copied to clipboard ✓')),
                        );
                      },
                    ),
                  ],
                ),
        ),
      ),
    );
  }
}
