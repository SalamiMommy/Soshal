// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:soshal_flutter/frb_generated.dart';

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

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    setState(() => _loading = true);
    try {
      final json = RustLib.instance.api.crateFfiDbDbStorageStats();
      final rows = jsonDecode(json) as List<dynamic>;
      _stats = rows.map((e) => e as Map<String, dynamic>).toList();
      _error = null;
    } catch (e) {
      _error = '$e';
    }
    try {
      final ad =
          RustLib.instance.api.crateFfiDbDbGetSetting(key: 'auto_download');
      if (ad != null) _autoDownload = ad.toLowerCase() == 'true';
      final ap = RustLib.instance.api.crateFfiDbDbGetSetting(key: 'auto_play');
      if (ap != null) _autoPlay = ap.toLowerCase() == 'true';
    } catch (_) {}
    if (mounted) setState(() => _loading = false);
  }

  Future<void> _setSetting(String key, bool value) async {
    try {
      RustLib.instance.api.crateFfiDbDbSetSetting(
        key: key,
        value: value.toString(),
      );
    } catch (e) {
      debugPrint('set $key: $e');
    }
  }

  Future<void> _clearOldPosts() async {
    final cutoff = DateTime.now()
            .subtract(const Duration(days: 30))
            .millisecondsSinceEpoch ~/
        1000;
    try {
      RustLib.instance.api.crateFfiDbDbExecuteRaw(
        sql:
            "UPDATE posts SET is_deleted = 1 WHERE is_deleted = 0 AND created_at < $cutoff",
      );
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(content: Text('Old posts cleared')),
        );
      }
      _load();
    } catch (e) {
      debugPrint('clear old posts: $e');
    }
  }

  Future<void> _clearAllPosts() async {
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
    try {
      RustLib.instance.api.crateFfiDbDbExecuteRaw(
        sql: "UPDATE posts SET is_deleted = 1 WHERE is_deleted = 0",
      );
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(content: Text('All posts cleared')),
        );
      }
      _load();
    } catch (e) {
      debugPrint('clear posts: $e');
    }
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
              Text('Error loading stats: $_error')
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
