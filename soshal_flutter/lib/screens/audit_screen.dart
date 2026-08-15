import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import '../services/audit_service.dart';
import '../utils/format.dart';
import '../widgets/error_state_text.dart';

/// Audit Log: read-only security event log stored locally in SQLite.
class AuditScreen extends StatefulWidget {
  /// Audit log screen.
  const AuditScreen({super.key});

  @override
  State<AuditScreen> createState() => _AuditScreenState();
}

class _AuditScreenState extends State<AuditScreen> {
  bool _loading = false;
  String? _error;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      await context.read<AuditService>().list(limit: 100);
    } catch (e) {
      debugPrint('audit list: $e');
      if (mounted) setState(() => _error = e.toString());
    }
    if (mounted) setState(() => _loading = false);
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: const Text('Audit Log'),
        actions: [
          IconButton(
            icon: const Icon(Icons.refresh),
            onPressed: _loading ? null : _load,
          ),
        ],
      ),
      body: Consumer<AuditService>(
        builder: (context, api, _) {
          if (_loading) {
            return const Center(child: CircularProgressIndicator());
          }
          if (_error != null) {
            return Center(
              child: Padding(
                padding: const EdgeInsets.all(16),
                child: ErrorStateText('Error: $_error'),
              ),
            );
          }
          final rows = api.rows;
          if (rows.isEmpty) {
            return const Center(
              child: Padding(
                padding: EdgeInsets.all(24),
                child: Column(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    Icon(Icons.shield_outlined, size: 48, color: Colors.grey),
                    SizedBox(height: 12),
                    Text('No audit entries yet'),
                    SizedBox(height: 4),
                    Text(
                      'Security events will appear here as they happen.',
                      style: TextStyle(color: Colors.grey),
                      textAlign: TextAlign.center,
                    ),
                  ],
                ),
              ),
            );
          }
          return ListView.builder(
            itemCount: rows.length,
            itemBuilder: (context, index) {
              final r = rows[index];
              final target = r.targetPubkey;
              return Card(
                margin: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
                child: ListTile(
                  leading: CircleAvatar(
                    radius: 16,
                    child: Text(
                      r.action.isNotEmpty ? r.action[0].toUpperCase() : '?',
                      style: const TextStyle(fontSize: 12),
                    ),
                  ),
                  title: Text(r.action,
                      maxLines: 1, overflow: TextOverflow.ellipsis),
                  subtitle: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text('actor: ${shortPubkey(r.actorPubkey)}'),
                      if (target != null && target.isNotEmpty)
                        Text('target: ${shortPubkey(target)}'),
                      if (r.details != null && r.details!.isNotEmpty)
                        Text(r.details!,
                            maxLines: 2, overflow: TextOverflow.ellipsis),
                    ],
                  ),
                  isThreeLine: true,
                  trailing: Text(
                    formatTimestamp(r.createdAt),
                    style: Theme.of(context).textTheme.bodySmall,
                  ),
                ),
              );
            },
          );
        },
      ),
    );
  }
}
