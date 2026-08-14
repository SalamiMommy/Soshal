import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import '../services/session_service.dart';
import '../services/streaming_service.dart';

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

  Future<void> _postStoryDialog() async {
    final text = TextEditingController();
    final images = TextEditingController();

    final ok = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
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
                hintText: 'comma separated URLs',
              ),
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
                      children: const [
                        SizedBox(height: 240),
                        Center(
                            child: Text('No stories from people you follow')),
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
                          backgroundColor:
                              viewed ? Colors.grey.shade400 : Colors.purple,
                          child:
                              const Icon(Icons.camera_alt, color: Colors.white),
                        ),
                        title: Text(
                          s.pubkey.length >= 12
                              ? s.pubkey.substring(0, 12)
                              : s.pubkey,
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
                        trailing: viewed
                            ? const Text('seen',
                                style:
                                    TextStyle(fontSize: 11, color: Colors.grey))
                            : null,
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
