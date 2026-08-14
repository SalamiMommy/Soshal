import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';
import '../services/session_service.dart';

/// Account management: list accounts, switch, remove, add new.
class AccountsScreen extends StatelessWidget {
  /// Account management screen.
  const AccountsScreen({super.key});

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Accounts')),
      body: Consumer<SessionService>(
        builder: (context, session, _) {
          final accounts = session.session?.accounts ?? [];
          if (accounts.isEmpty) {
            return Center(
              child: Column(
                mainAxisAlignment: MainAxisAlignment.center,
                children: [
                  const Text('No accounts yet'),
                  const SizedBox(height: 16),
                  FilledButton(
                    onPressed: () => context.go('/auth'),
                    child: const Text('Add account'),
                  ),
                ],
              ),
            );
          }
          return ListView(
            children: [
              for (final account in accounts)
                ListTile(
                  leading: account.pubkey == session.activePubkey
                      ? const Icon(Icons.check_circle, color: Colors.green)
                      : const Icon(Icons.account_circle),
                  title: Text(
                    account.npub.isNotEmpty
                        ? account.npub
                        : account.pubkey.substring(0, 16),
                  ),
                  subtitle: Text(account.pubkey.substring(0, 24)),
                  trailing: Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      if (account.pubkey != session.activePubkey)
                        TextButton(
                          onPressed: () async {
                            await session.switchAccount(account.pubkey);
                          },
                          child: const Text('Switch'),
                        ),
                      IconButton(
                        icon: const Icon(Icons.delete_outline),
                        tooltip: 'Remove account',
                        onPressed: () async {
                          final confirmed = await showDialog<bool>(
                            context: context,
                            builder: (context) => AlertDialog(
                              title: const Text('Remove account?'),
                              content: const Text(
                                  'Key material stays in the OS keychain. Relogin '
                                  'restores it via your recovery phrase.'),
                              actions: [
                                TextButton(
                                  onPressed: () =>
                                      Navigator.pop(context, false),
                                  child: const Text('Cancel'),
                                ),
                                FilledButton(
                                  onPressed: () => Navigator.pop(context, true),
                                  child: const Text('Remove'),
                                ),
                              ],
                            ),
                          );
                          if (confirmed == true) {
                            await session.removeAccount(account.pubkey);
                          }
                        },
                      ),
                    ],
                  ),
                ),
              const Divider(),
              ListTile(
                leading: const Icon(Icons.add),
                title: const Text('Add account'),
                onTap: () => context.go('/auth'),
              ),
            ],
          );
        },
      ),
    );
  }
}
