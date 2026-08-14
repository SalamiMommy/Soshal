import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:provider/provider.dart';
import '../services/music_service.dart';

/// Musicloud user page: one author's published tracks (kind 31022).
class MusicloudUserScreen extends StatefulWidget {
  /// Author pubkey being listed.
  final String pubkey;

  /// Musicloud user screen.
  const MusicloudUserScreen({super.key, required this.pubkey});

  @override
  State<MusicloudUserScreen> createState() => _MusicloudUserScreenState();
}

class _MusicloudUserScreenState extends State<MusicloudUserScreen> {
  bool _loading = true;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    setState(() => _loading = true);
    try {
      await context.read<MusicService>().fetchTracks(author: widget.pubkey);
    } catch (e) {
      debugPrint('musicloud user load: $e');
    }
    if (mounted) setState(() => _loading = false);
  }

  void _openTrack(MusicTrack track) {
    showModalBottomSheet<void>(
      context: context,
      builder: (sheetContext) => SafeArea(
        child: Padding(
          padding: const EdgeInsets.all(16),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(
                track.title.isEmpty ? 'Untitled track' : track.title,
                style: Theme.of(sheetContext).textTheme.titleLarge,
              ),
              const SizedBox(height: 4),
              Text(
                '${_shortPk(track.pubkey)} · ${_fmtTime(track.createdAt)}',
                style: Theme.of(sheetContext).textTheme.bodySmall,
              ),
              const SizedBox(height: 16),
              FilledButton.icon(
                onPressed: () {
                  Clipboard.setData(ClipboardData(text: track.audioUrl));
                  Navigator.of(sheetContext).pop();
                  if (!context.mounted) return;
                  ScaffoldMessenger.of(context).showSnackBar(
                    const SnackBar(
                        content: Text('Audio URL copied to clipboard')),
                  );
                },
                icon: const Icon(Icons.copy),
                label: const Text('Copy audio URL'),
              ),
            ],
          ),
        ),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final tracks = context.watch<MusicService>().tracks;
    final error = context.watch<MusicService>().lastError;
    return Scaffold(
      appBar: AppBar(title: const Text('Music')),
      body: _loading
          ? const Center(child: CircularProgressIndicator())
          : RefreshIndicator(
              onRefresh: _load,
              child: tracks.isEmpty
                  ? ListView(
                      children: [
                        const SizedBox(height: 120),
                        Center(
                          child: Padding(
                            padding: const EdgeInsets.all(24),
                            child: Column(
                              children: [
                                Icon(
                                  Icons.music_note,
                                  size: 56,
                                  color: Theme.of(context).colorScheme.outline,
                                ),
                                const SizedBox(height: 16),
                                Text('No tracks published',
                                    style:
                                        Theme.of(context).textTheme.titleLarge),
                                const SizedBox(height: 8),
                                Text(
                                  'This author has not published any Musicloud tracks yet.',
                                  textAlign: TextAlign.center,
                                  style: TextStyle(
                                      color: Theme.of(context)
                                          .colorScheme
                                          .onSurfaceVariant),
                                ),
                              ],
                            ),
                          ),
                        ),
                      ],
                    )
                  : ListView.separated(
                      itemCount: tracks.length,
                      separatorBuilder: (_, __) => const Divider(height: 1),
                      itemBuilder: (context, index) {
                        final track = tracks[index];
                        return ListTile(
                          leading: track.thumbnail.isNotEmpty
                              ? ClipRRect(
                                  borderRadius: BorderRadius.circular(8),
                                  child: Image.network(
                                    track.thumbnail,
                                    width: 48,
                                    height: 48,
                                    fit: BoxFit.cover,
                                    errorBuilder: (_, __, ___) =>
                                        _trackIcon(context),
                                  ),
                                )
                              : _trackIcon(context),
                          title: Text(
                            track.title.isEmpty
                                ? 'Untitled track'
                                : track.title,
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                          ),
                          subtitle: Text(
                            [
                              _shortPk(track.pubkey),
                              _fmtTime(track.createdAt),
                              if (track.hashtags.isNotEmpty)
                                track.hashtags.map((h) => '#$h').join(' '),
                            ].join(' · '),
                            maxLines: 2,
                            overflow: TextOverflow.ellipsis,
                          ),
                          trailing: IconButton(
                            icon: const Icon(Icons.open_in_new),
                            tooltip: 'Track URL',
                            onPressed: () => _openTrack(track),
                          ),
                          onTap: () => _openTrack(track),
                        );
                      },
                    ),
            ),
      bottomNavigationBar: error != null && error.isNotEmpty
          ? Material(
              color: Theme.of(context).colorScheme.errorContainer,
              child: Padding(
                padding: const EdgeInsets.all(12),
                child: Text(
                  'Error: $error',
                  style: TextStyle(
                    color: Theme.of(context).colorScheme.onErrorContainer,
                  ),
                ),
              ),
            )
          : null,
    );
  }

  Widget _trackIcon(BuildContext context) => Container(
        width: 48,
        height: 48,
        decoration: BoxDecoration(
          color: Theme.of(context).colorScheme.surfaceContainerHighest,
          borderRadius: BorderRadius.circular(8),
        ),
        child: Icon(Icons.music_note,
            color: Theme.of(context).colorScheme.primary),
      );

  static String _shortPk(String pk) => pk.length <= 12
      ? pk
      : '${pk.substring(0, 6)}…${pk.substring(pk.length - 6)}';

  static String _fmtTime(int seconds) {
    if (seconds <= 0) return '';
    final dt = DateTime.fromMillisecondsSinceEpoch(seconds * 1000);
    final now = DateTime.now();
    final diff = now.difference(dt);
    if (diff.inDays > 0) return '${diff.inDays}d ago';
    if (diff.inHours > 0) return '${diff.inHours}h ago';
    if (diff.inMinutes > 0) return '${diff.inMinutes}m ago';
    return 'just now';
  }
}
