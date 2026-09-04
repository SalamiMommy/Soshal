import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import '../services/media_service.dart';
import '../services/session_service.dart';
import '../services/streaming_service.dart';
import '../utils/format.dart';
import '../widgets/empty_state.dart';

/// Stories: followed authors' stories, mark viewed, post your own.
class StoriesScreen extends StatefulWidget {
  /// Stories screen.
  const StoriesScreen({super.key});

  @override
  State<StoriesScreen> createState() => _StoriesScreenState();
}

class _StoriesScreenState extends State<StoriesScreen> {
  bool _loading = true;
  final Map<String, bool> _viewed = {};

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    setState(() => _loading = true);
    try {
      final api = context.read<StreamingService>();
      final session = context.read<SessionService>();
      final pubkey = session.activePubkey;
      if (pubkey != null) {
        await api.fetchFollowedStories(pubkey);
      } else {
        await api.fetchStories('');
      }
      _viewed.clear();
      for (final s in api.stories) {
        _viewed[s.id] = false;
      }
    } catch (e) {
      debugPrint('stories load: $e');
    }
    if (mounted) setState(() => _loading = false);
  }

  Future<void> _view(StreamRow story) async {
    if (_viewed[story.id] ?? false) return;
    try {
      final session = context.read<SessionService>();
      final pubkey = session.activePubkey;
      if (pubkey == null) return;
      await context.read<StreamingService>().markStoryViewed(story.id, pubkey);
      setState(() => _viewed[story.id] = true);
    } catch (e) {
      debugPrint('mark viewed: $e');
    }
  }

  Future<void> _react(StreamRow story) async {
    const emojis = ['❤️', '😂', '🔥', '👍'];
    final emoji = await showDialog<String>(
      context: context,
      builder: (context) => SimpleDialog(
        title: const Text('React to story'),
        children: [
          for (final e in emojis)
            SimpleDialogOption(
              onPressed: () => Navigator.pop(context, e),
              child: Text(e, style: const TextStyle(fontSize: 24)),
            ),
        ],
      ),
    );
    if (emoji == null) return;
    if (!mounted) return;
    try {
      final session = context.read<SessionService>();
      final pubkey = session.activePubkey;
      if (pubkey == null) return;
      final ok = await context
          .read<StreamingService>()
          .storyReact(story.id, pubkey, emoji);
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
            content: SelectableText(ok ? 'Reacted $emoji' : 'React failed')),
      );
    } catch (e) {
      debugPrint('story react: $e');
    }
  }

  Future<void> _postStoryDialog() async {
    final text = TextEditingController();
    final images = TextEditingController();
    bool uploading = false;

    final ok = await showDialog<bool>(
      context: context,
      builder: (context) => StatefulBuilder(
        builder: (context, setDialogState) => AlertDialog(
          title: const Text('Post a story'),
          content: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              TextField(
                controller: text,
                maxLines: 3,
                decoration: const InputDecoration(labelText: 'Story text'),
              ),
              TextField(
                controller: images,
                decoration: const InputDecoration(
                  labelText: 'Images',
                  hintText: 'pick from device or paste URLs',
                ),
              ),
              const SizedBox(height: 8),
              OutlinedButton.icon(
                onPressed: uploading
                    ? null
                    : () async {
                        try {
                          final picked =
                              await FilePicker.pickFile(type: FileType.image);
                          final path = picked?.path;
                          if (path == null || !context.mounted) return;
                          setDialogState(() => uploading = true);
                          final manifest = await context
                              .read<MediaService>()
                              .uploadMedia(path);
                          final hash = manifest['blob_hash'] as String? ?? '';
                          if (hash.length != 64) {
                            throw Exception('Bad upload manifest');
                          }
                          final list = images.text
                              .split(',')
                              .map((e) => e.trim())
                              .where((e) => e.isNotEmpty)
                              .toList()
                            ..add('n$hash');
                          images.text = list.join(', ');
                        } catch (e) {
                          if (context.mounted) {
                            ScaffoldMessenger.of(context).showSnackBar(SnackBar(
                                content: SelectableText('Upload error: $e')));
                          }
                        } finally {
                          setDialogState(() => uploading = false);
                        }
                      },
                icon: uploading
                    ? const SizedBox(
                        width: 16,
                        height: 16,
                        child: CircularProgressIndicator(strokeWidth: 2),
                      )
                    : const Icon(Icons.add_photo_alternate_outlined),
                label: const Text('Pick image from device'),
              ),
            ],
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.pop(context, false),
              child: const Text('Cancel'),
            ),
            FilledButton(
              onPressed: () => Navigator.pop(context, true),
              child: const Text('Post'),
            ),
          ],
        ),
      ),
    );

    if (ok == true) {
      if (!mounted) return;
      try {
        final session = context.read<SessionService>();
        final pubkey = session.activePubkey;
        if (pubkey == null) throw Exception('Sign in to post');
        final imageList = images.text
            .split(',')
            .map((e) => e.trim())
            .where((e) => e.isNotEmpty)
            .toList();
        await context.read<StreamingService>().postStory(
              pubkey,
              text.text.trim(),
              imageList,
              24,
            );
        await _load();
      } catch (e) {
        if (mounted) {
          ScaffoldMessenger.of(context).showSnackBar(
              SnackBar(content: SelectableText('Post failed: $e')));
        }
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Stories')),
      floatingActionButton: FloatingActionButton(
        onPressed: _postStoryDialog,
        tooltip: 'Post story',
        child: const Icon(Icons.add),
      ),
      body: _loading
          ? const Center(child: CircularProgressIndicator())
          : Consumer<StreamingService>(
              builder: (context, api, _) {
                if (api.stories.isEmpty) {
                  return RefreshIndicator(
                    onRefresh: _load,
                    child: ListView(
                      physics: const AlwaysScrollableScrollPhysics(),
                      children: [
                        const SizedBox(height: 120),
                        EmptyState(
                          icon: Icons.auto_stories_outlined,
                          title: 'No stories from people you follow',
                        ),
                      ],
                    ),
                  );
                }
                return RefreshIndicator(
                  onRefresh: _load,
                  child: ListView.builder(
                    itemCount: api.stories.length,
                    itemBuilder: (context, index) {
                      final s = api.stories[index];
                      final viewed = _viewed[s.id] ?? false;
                      return ListTile(
                        leading: CircleAvatar(
                          backgroundColor: viewed
                              ? Theme.of(context).colorScheme.outlineVariant
                              : Theme.of(context).colorScheme.primary,
                          child:
                              const Icon(Icons.camera_alt, color: Colors.white),
                        ),
                        title: Text(
                          firstChars(s.pubkey, 12),
                          style: const TextStyle(fontSize: 13),
                        ),
                        subtitle: Text(
                          (s.summary.isNotEmpty ? s.summary : s.title),
                          maxLines: 2,
                          overflow: TextOverflow.ellipsis,
                          style: TextStyle(
                            color: viewed
                                ? Colors.grey
                                : Theme.of(context).colorScheme.primary,
                          ),
                        ),
                        trailing: Row(
                          mainAxisSize: MainAxisSize.min,
                          children: [
                            if (viewed)
                              const Text('seen',
                                  style: TextStyle(
                                      fontSize: 11, color: Colors.grey)),
                            IconButton(
                              icon: const Icon(Icons.add_reaction_outlined),
                              tooltip: 'React',
                              onPressed: () => _react(s),
                            ),
                          ],
                        ),
                        onTap: () => _view(s),
                      );
                    },
                  ),
                );
              },
            ),
    );
  }
}
