// ignore_for_file: invalid_use_of_internal_member
import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import 'package:soshal_flutter/services/media_service.dart';
import 'package:soshal_flutter/services/settings_service.dart';
import '../widgets/error_state_text.dart';

/// Storage settings — database size, per-table row counts (via the
/// `db_storage_stats` FFI), cache clearing, and auto-download toggles
/// persisted as settings keys (port of the legacy storage section).
class StorageScreen extends StatefulWidget {
  const StorageScreen({super.key});

  @override
  State<StorageScreen> createState() => _StorageScreenState();
}

class _StorageScreenState extends State<StorageScreen> {
  List<Map<String, dynamic>> _stats = [];
  bool _loading = true;
  String? _error;
  bool _autoDownload = true;
  bool _autoPlay = true;
  String? _cachePath;
  int? _serverPort;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    setState(() => _loading = true);
    try {
      _stats = context.read<SettingsService>().storageStats();
      _error = null;
    } catch (e) {
      _error = '$e';
    }
    try {
      final media = context.read<MediaService>();
      _cachePath = await media.getCachePath();
      _serverPort = media.localServerPort;
    } catch (e) {
      debugPrint('cache path: $e');
    }
    try {
      final settings = context.read<SettingsService>();
      final ad = settings.getSetting('auto_download');
      if (ad.isNotEmpty) _autoDownload = ad.toLowerCase() == 'true';
      final ap = settings.getSetting('auto_play');
      if (ap.isNotEmpty) _autoPlay = ap.toLowerCase() == 'true';
    } catch (_) {}
    if (mounted) setState(() => _loading = false);
  }

  Future<void> _clearCache() async {
    final media = context.read<MediaService>();
    await media.clearCache();
    if (mounted) {
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(content: Text('Media cache cleared')),
      );
    }
    _load();
  }

  Future<void> _startServer() async {
    try {
      final port = await context.read<MediaService>().startLocalServer();
      if (mounted) setState(() => _serverPort = port);
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text('Server start failed: $e')),
        );
      }
    }
  }

  Future<void> _stopServer() async {
    await context.read<MediaService>().stopLocalServer();
    if (mounted) setState(() => _serverPort = null);
  }

  Future<void> _setSetting(String key, bool value) async {
    try {
      context.read<SettingsService>().setSetting(key, value.toString());
    } catch (e) {
      debugPrint('set $key: $e');
    }
  }

  Future<void> _clearOldPosts() async {
    final ok = await context.read<SettingsService>().purgeOldPosts();
    if (!ok) return;
    if (mounted) {
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(content: Text('Old posts cleared')),
      );
    }
    _load();
  }

  Future<void> _clearAllPosts() async {
    final settings = context.read<SettingsService>();
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: const Text('Clear all posts?'),
        content: const Text(
            'Removes all locally cached posts. This cannot be undone.'),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(ctx).pop(false),
            child: const Text('Cancel'),
          ),
          FilledButton(
            onPressed: () => Navigator.of(ctx).pop(true),
            child: const Text('Clear'),
          ),
        ],
      ),
    );
    if (confirmed != true) return;
    final ok = await settings.purgeAllPosts();
    if (!ok) return;
    if (mounted) {
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(content: Text('All posts cleared')),
      );
    }
    _load();
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Storage')),
      body: RefreshIndicator(
        onRefresh: _load,
        child: ListView(
          padding: const EdgeInsets.all(16),
          children: [
            if (_loading)
              const Center(
                  child: Padding(
                padding: EdgeInsets.all(24),
                child: CircularProgressIndicator(),
              ))
            else if (_error != null)
              ErrorStateText('Error loading stats: $_error')
            else ...[
              Text('Database', style: Theme.of(context).textTheme.titleMedium),
              const SizedBox(height: 8),
              for (final row in _stats)
                ListTile(
                  dense: true,
                  leading: const Icon(Icons.table_chart_outlined),
                  title: Text(row['table_name'] as String? ?? ''),
                  subtitle: row['table_name'] == '__db_file__'
                      ? null
                      : Text('rows: ${row['rows'] ?? 0}'),
                  trailing: row['table_name'] == '__db_file__'
                      ? Text(_fmtBytes(row['bytes'] as int? ?? 0))
                      : null,
                ),
              if (_stats.any((r) => r['table_name'] == '__db_file__'))
                ListTile(
                  leading: const Icon(Icons.storage_outlined),
                  title: const Text('Database file'),
                  trailing: Text(
                    _fmtBytes(_stats.firstWhere(
                          (r) => r['table_name'] == '__db_file__',
                        )['bytes'] as int? ??
                        0),
                  ),
                ),
            ],
            const Divider(),
            SwitchListTile(
              title: const Text('Auto-download media'),
              value: _autoDownload,
              onChanged: (v) {
                setState(() => _autoDownload = v);
                _setSetting('auto_download', v);
              },
            ),
            SwitchListTile(
              title: const Text('Auto-play media'),
              value: _autoPlay,
              onChanged: (v) {
                setState(() => _autoPlay = v);
                _setSetting('auto_play', v);
              },
            ),
            const Divider(),
            ListTile(
              leading: const Icon(Icons.delete_sweep_outlined),
              title: const Text('Clear old posts'),
              subtitle: const Text('Delete cached posts older than 30 days'),
              onTap: _clearOldPosts,
            ),
            ListTile(
              leading: const Icon(Icons.delete_forever_outlined),
              title: const Text('Clear all posts'),
              subtitle: const Text('Delete every locally cached post'),
              onTap: _clearAllPosts,
            ),
            const Divider(),
            ListTile(
              leading: const Icon(Icons.folder_open_outlined),
              title: const Text('Cache path'),
              subtitle: Text(
                _cachePath ?? '…',
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
              ),
            ),
            ListTile(
              leading: const Icon(Icons.cleaning_services_outlined),
              title: const Text('Clear media cache'),
              subtitle: const Text('Evict all stored media chunks'),
              onTap: _clearCache,
            ),
            ListTile(
              leading: const Icon(Icons.dns_outlined),
              title: const Text('Local media server'),
              subtitle: Text(
                _serverPort != null
                    ? 'Running on port $_serverPort'
                    : 'Not running',
              ),
              trailing: TextButton(
                onPressed: _serverPort == null ? _startServer : _stopServer,
                child: Text(_serverPort == null ? 'Start' : 'Stop'),
              ),
            ),
          ],
        ),
      ),
    );
  }

  String _fmtBytes(int b) {
    if (b >= 1 << 30) return '${(b / (1 << 30)).toStringAsFixed(1)} GiB';
    if (b >= 1 << 20) return '${(b / (1 << 20)).toStringAsFixed(1)} MiB';
    if (b >= 1 << 10) return '${(b / (1 << 10)).toStringAsFixed(1)} KiB';
    return '$b B';
  }
}
