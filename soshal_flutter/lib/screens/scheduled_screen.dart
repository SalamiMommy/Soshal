import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import '../services/scheduled_service.dart';
import '../services/session_service.dart';

/// Scheduled Posts: draft posts with a future scheduled_at, broadcast by
/// the sync pipeline when due.
class ScheduledScreen extends StatefulWidget {
  /// Scheduled posts screen.
  const ScheduledScreen({super.key});

  @override
  State<ScheduledScreen> createState() => _ScheduledScreenState();
}

class _ScheduledScreenState extends State<ScheduledScreen> {
  bool _loading = true;
  String? _pubkey;

  @override
  void initState() {
    super.initState();
    _pubkey = context.read<SessionService>().activePubkey;
    _load();
  }

  Future<void> _load() async {
    final pubkey = _pubkey;
    if (pubkey == null) {
      if (mounted) setState(() => _loading = false);
      return;
    }
    try {
      await context.read<ScheduledService>().list(pubkey);
    } catch (e) {
      debugPrint('scheduled load: $e');
    }
    if (mounted) setState(() => _loading = false);
  }

  Future<void> _composeDialog() async {
    final content = TextEditingController();
    final hashtags = TextEditingController();
    final now = DateTime.now();
    var when = now.add(const Duration(hours: 1));

    final ok = await showDialog<bool>(
      context: context,
      builder: (context) => StatefulBuilder(
        builder: (context, setDialogState) => AlertDialog(
          title: const Text('Schedule a post'),
          content: SingleChildScrollView(
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                TextField(
                  controller: content,
                  maxLines: 4,
                  decoration: const InputDecoration(
                    labelText: 'Content *',
                    border: OutlineInputBorder(),
                  ),
                ),
                const SizedBox(height: 12),
                TextField(
                  controller: hashtags,
                  decoration: const InputDecoration(
                    labelText: 'Hashtags',
                    hintText: '#music #tech',
                    border: OutlineInputBorder(),
                  ),
                ),
                const SizedBox(height: 12),
                ListTile(
                  contentPadding: EdgeInsets.zero,
                  leading: const Icon(Icons.schedule),
                  title: const Text('Schedule for'),
                  trailing: Text(_fmt(when)),
                  onTap: () async {
                    final picked = await showDatePicker(
                      context: context,
                      initialDate: when,
                      firstDate: now,
                      lastDate: now.add(const Duration(days: 365)),
                    );
                    if (picked == null) return;
                    if (!context.mounted) return;
                    final time = await showTimePicker(
                      context: context,
                      initialTime: TimeOfDay.fromDateTime(when),
                    );
                    if (time == null) return;
                    setDialogState(() {
                      when = DateTime(picked.year, picked.month, picked.day,
                          time.hour, time.minute);
                    });
                  },
                ),
              ],
            ),
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.pop(context, false),
              child: const Text('Cancel'),
            ),
            FilledButton(
              onPressed: () => Navigator.pop(context, true),
              child: const Text('Schedule'),
            ),
          ],
        ),
      ),
    );

    if (ok != true) return;
    if (!mounted) return;
    final pubkey = _pubkey;
    if (pubkey == null) {
      _showSnack('Sign in required to schedule posts');
      return;
    }
    if (content.text.trim().isEmpty) {
      _showSnack('Content required');
      return;
    }
    final tags = hashtags.text
        .split(RegExp(r'[,\s]+'))
        .map((s) => s.trim().replaceFirst('#', ''))
        .where((s) => s.isNotEmpty)
        .toList();
    try {
      await context.read<ScheduledService>().create(
            pubkey: pubkey,
            content: content.text.trim(),
            scheduledAt: when.millisecondsSinceEpoch ~/ 1000,
            hashtags: tags,
          );
      await _load();
    } catch (e) {
      if (mounted) _showSnack('Schedule failed: $e');
    }
  }

  Future<void> _delete(ScheduledPost post) async {
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Cancel this scheduled post?'),
        content: const Text('The draft will be removed.'),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: const Text('Keep'),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, true),
            child: const Text('Yes, cancel'),
          ),
        ],
      ),
    );
    if (confirmed != true) return;
    if (!mounted) return;
    try {
      await context.read<ScheduledService>().delete(post.id);
      await _load();
      if (mounted) _showSnack('Cancelled.');
    } catch (e) {
      if (mounted) _showSnack('Cancel failed: $e');
    }
  }

  void _showSnack(String message) {
    ScaffoldMessenger.of(context)
        .showSnackBar(SnackBar(content: SelectableText(message)));
  }

  String _fmt(DateTime t) {
    String two(int v) => v.toString().padLeft(2, '0');
    return '${t.year}-${two(t.month)}-${two(t.day)} ${two(t.hour)}:${two(t.minute)}';
  }

  String _fmtTs(int ts) {
    if (ts <= 0) return 'unscheduled';
    return _fmt(DateTime.fromMillisecondsSinceEpoch(ts * 1000).toLocal());
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: const Text('Scheduled Posts'),
        actions: [
          IconButton(
            icon: const Icon(Icons.add),
            tooltip: 'Schedule a post',
            onPressed: _pubkey == null ? null : _composeDialog,
          ),
        ],
      ),
      body: _pubkey == null
          ? const Center(child: Text('Sign in required'))
          : _loading
              ? const Center(child: CircularProgressIndicator())
              : Consumer<ScheduledService>(
                  builder: (context, service, _) {
                    if (service.drafts.isEmpty) {
                      return const Center(
                          child: Text('No scheduled posts. Tap + to add one.'));
                    }
                    return RefreshIndicator(
                      onRefresh: _load,
                      child: ListView.builder(
                        padding: const EdgeInsets.all(8),
                        itemCount: service.drafts.length,
                        itemBuilder: (context, index) {
                          final post = service.drafts[index];
                          return Card(
                            margin: const EdgeInsets.symmetric(
                                vertical: 4, horizontal: 4),
                            child: ListTile(
                              title: Text(post.content,
                                  maxLines: 3, overflow: TextOverflow.ellipsis),
                              subtitle: Text(
                                '${_fmtTs(post.scheduledAt ?? 0)}'
                                '${post.mentionedHashtags.isEmpty ? '' : ' · #${post.mentionedHashtags.join(' #')}'}',
                                maxLines: 1,
                                overflow: TextOverflow.ellipsis,
                              ),
                              trailing: IconButton(
                                icon: const Icon(Icons.delete_outline),
                                tooltip: 'Cancel',
                                onPressed: () => _delete(post),
                              ),
                            ),
                          );
                        },
                      ),
                    );
                  },
                ),
    );
  }
}
