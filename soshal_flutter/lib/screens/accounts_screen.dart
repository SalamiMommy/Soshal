import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';
import '../services/session_service.dart';
import '../services/signer_service.dart';
import '../utils/format.dart';
import '../widgets/empty_state.dart';

/// Account management: list accounts, switch, remove, add new.
class AccountsScreen extends StatefulWidget {
  /// Account management screen.
  const AccountsScreen({super.key});

  @override
  State<AccountsScreen> createState() => _AccountsScreenState();
}

class _AccountsScreenState extends State<AccountsScreen> {
  bool _refreshing = false;

  Future<void> _refreshFromRust() async {
    setState(() => _refreshing = true);
    try {
      final session = context.read<SessionService>();
      final refreshed = await session.refreshFromRust();
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: SelectableText(
            refreshed == null
                ? 'No Rust session to sync'
                : 'Synced ${refreshed.accounts.length} account(s) from Rust',
          ),
        ),
      );
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: SelectableText('Refresh error: $e')),
        );
      }
    } finally {
      if (mounted) setState(() => _refreshing = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: const Text('Accounts'),
        actions: [
          IconButton(
            icon: _refreshing
                ? const SizedBox(
                    width: 18,
                    height: 18,
                    child: CircularProgressIndicator(strokeWidth: 2),
                  )
                : const Icon(Icons.sync),
            tooltip: 'Refresh from Rust',
            onPressed: _refreshing ? null : _refreshFromRust,
          ),
        ],
      ),
      body: Consumer<SessionService>(
        builder: (context, session, _) {
          final accounts = session.getAccounts();
          if (accounts.isEmpty) {
            return EmptyState(
              icon: Icons.person_add_alt_1,
              title: 'No accounts yet',
              action: FilledButton(
                onPressed: () => context.go('/auth'),
                child: const Text('Add account'),
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
                        : prefixEllipsis(account.pubkey, 16),
                  ),
                  subtitle: Text(prefixEllipsis(account.pubkey, 24)),
                  trailing: Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      if (account.pubkey != session.activePubkey)
                        TextButton(
                          onPressed: () async {
                            await session.switchAccount(account.pubkey);
                            if (!context.mounted) return;
                            // Re-key the in-process signer for the new
                            // account; when no keychain entry exists, lock
                            // it so the signer-lock overlay (recovery phrase
                            // verified against the now-active account) shows
                            // instead of mismatched-pubkey sign errors.
                            final signer = context.read<SignerService>();
                            var unlocked = false;
                            try {
                              unlocked = await signer
                                  .unlockFromKeyring(account.pubkey);
                            } catch (_) {}
                            if (!unlocked) {
                              await signer.lock();
                            }
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
