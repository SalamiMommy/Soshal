// ignore_for_file: invalid_use_of_internal_member
import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import '../services/backup_service.dart';
import '../services/error_log.dart';
import '../services/session_service.dart';
import '../services/settings_service.dart';

/// Backup screen: account identifiers + recovery guidance.
class BackupScreen extends StatefulWidget {
  /// Backup screen.
  const BackupScreen({super.key});

  @override
  State<BackupScreen> createState() => _BackupScreenState();
}

class _BackupScreenState extends State<BackupScreen> {
  late final Future<String> _dbPathFuture;
  late final Future<Map<String, int>> _countsFuture;

  @override
  void initState() {
    super.initState();
    _dbPathFuture = Future.value(context.read<SettingsService>().dbPath());
    _countsFuture = _counts();
  }

  Future<Map<String, int>> _counts() async {
    const tables = ['posts', 'messages', 'notifications', 'users', 'zaps'];
    final out = <String, int>{};
    for (final t in tables) {
      try {
        final c = context.read<BackupService>().dbCount(t);
        if (c > 0) out[t] = c.toInt();
      } catch (e, st) {
        debugPrint('backup table count failed: $e');
        logRuntimeError('backup table count: $e', st);
      }
    }
    return out;
  }

  @override
  Widget build(BuildContext context) {
    final session = context.watch<SessionService>();
    final account = session.activeAccount;

    return Scaffold(
      appBar: AppBar(title: const Text('Backup')),
      body: account == null || account.pubkey.isEmpty
          ? const Center(child: Text('Sign in to view backup info'))
          : ListView(
              padding: const EdgeInsets.all(16),
              children: [
                const Card(
                  child: Padding(
                    padding: EdgeInsets.all(16),
                    child: Row(
                      children: [
                        Icon(Icons.warning_amber, color: Colors.amber),
                        SizedBox(width: 12),
                        Expanded(
                          child: Text(
                            'Your secret key lives only in this device\'s OS '
                            'keychain and the soshal bridge. Soshal cannot '
                            'recover your account. Store your 24-word recovery '
                            'phrase (shown at account creation) somewhere safe.',
                          ),
                        ),
                      ],
                    ),
                  ),
                ),
                const SizedBox(height: 16),
                ListTile(
                  leading: const Icon(Icons.key),
                  title: const Text('Public key (hex)'),
                  subtitle: SelectableText(account.pubkey),
                ),
                ListTile(
                  leading: const Icon(Icons.alternate_email),
                  title: const Text('NIP-19 npub'),
                  subtitle: SelectableText(account.npub),
                ),
                ListTile(
                  leading: const Icon(Icons.group),
                  title: const Text('Relays'),
                  subtitle: Text(account.relayList.join(', ')),
                ),
                const Divider(),
                const ListTile(
                  leading: Icon(Icons.storage),
                  title: Text('Local database'),
                  subtitle:
                      Text('Export copies the SQLite store beside the live DB; '
                          'restore closes and replaces it, then reopens.'),
                ),
                FutureBuilder<String>(
                  future: _dbPathFuture,
                  builder: (context, snapshot) => ListTile(
                    leading: const Icon(Icons.folder_outlined),
                    title: const Text('Database path'),
                    subtitle:
                        snapshot.data == null || snapshot.data!.trim().isEmpty
                            ? const Text('…')
                            : Text(snapshot.data!,
                                maxLines: 1, overflow: TextOverflow.ellipsis),
                  ),
                ),
                FutureBuilder<Map<String, int>>(
                  future: _countsFuture,
                  builder: (context, snapshot) {
                    final counts = snapshot.data ?? {};
                    if (counts.isEmpty) return const SizedBox.shrink();
                    return Card(
                      margin: const EdgeInsets.symmetric(vertical: 8),
                      child: Padding(
                        padding: const EdgeInsets.all(12),
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            Text('Stored locally',
                                style: Theme.of(context).textTheme.titleSmall),
                            const SizedBox(height: 8),
                            Wrap(
                              spacing: 16,
                              runSpacing: 8,
                              children: [
                                for (final entry in counts.entries)
                                  Text(
                                    '${entry.key}: ${entry.value}',
                                    style: const TextStyle(fontSize: 12),
                                  ),
                              ],
                            ),
                          ],
                        ),
                      ),
                    );
                  },
                ),
                Padding(
                  padding: const EdgeInsets.all(16),
                  child: Row(
                    children: [
                      Expanded(
                        child: FilledButton.tonalIcon(
                          onPressed: () => _export(context),
                          icon: const Icon(Icons.upload_file),
                          label: const Text('Export backup'),
                        ),
                      ),
                      const SizedBox(width: 12),
                      Expanded(
                        child: OutlinedButton.icon(
                          onPressed: () => _restore(context),
                          icon: const Icon(Icons.restore),
                          label: const Text('Restore'),
                        ),
                      ),
                    ],
                  ),
                ),
              ],
            ),
    );
  }

  Future<void> _export(BuildContext context) async {
    final backupService = context.read<BackupService>();
    try {
      final backupPath = await backupService.getBackupPath();
      final result = await backupService.backup(backupPath);
      if (context.mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: SelectableText('Backup written: $result')),
        );
      }
    } catch (e) {
      if (context.mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: SelectableText('Export failed: $e')),
        );
      }
    }
  }

  Future<void> _restore(BuildContext context) async {
    final backupService = context.read<BackupService>();
    try {
      final backupPath = await backupService.getBackupPath();
      await backupService.restore(backupPath);
      if (context.mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(content: SelectableText('Restored from backup')),
        );
      }
    } catch (e) {
      if (context.mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: SelectableText('Restore failed: $e')),
        );
      }
    }
  }
}
