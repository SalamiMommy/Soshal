// ignore_for_file: invalid_use_of_internal_member

import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import '../services/feed_service.dart';
import '../frb_generated.dart';
import '../services/media_service.dart';
import '../services/search_service.dart';
import '../services/session_service.dart';
import '../services/signer_service.dart';

/// Composer Screen
/// Create post, add media/tags
class ComposerScreen extends StatefulWidget {
  const ComposerScreen({super.key});

  @override
  State<ComposerScreen> createState() => _ComposerScreenState();
}

class _ComposerScreenState extends State<ComposerScreen> {
  final _contentController = TextEditingController();
  final List<String> _tags = [];
  final List<String> _mentions = [];
  List<String> _detectedTags = [];
  bool _isPosting = false;
  PostMedia? _pendingMedia;
  bool _uploadingMedia = false;

  void _onContentChanged() {
    final detected = RustLib.instance.api
        .crateFfiContentContentExtractHashtags(text: _contentController.text);
    setState(() => _detectedTags = detected);
  }

  @override
  void dispose() {
    _contentController.dispose();
    super.dispose();
  }

  Future<void> _attachMedia() async {
    try {
      final result = await FilePicker.pickFiles(
        type: FileType.any,
        allowMultiple: false,
      );
      final path = result?.files.single.path;
      if (path == null || !mounted) return;
      setState(() => _uploadingMedia = true);
      final manifest = await context.read<MediaService>().uploadMedia(path);
      final blobHash = manifest['blob_hash'] as String? ?? '';
      if (blobHash.length != 64) {
        throw Exception('Upload failed (bad manifest)');
      }
      final ext = path.split('.').last.toLowerCase();
      final type =
          const ['jpg', 'jpeg', 'png', 'gif', 'webp', 'heic'].contains(ext)
              ? 'image'
              : const ['mp4', 'mov', 'mkv', 'webm', 'avi'].contains(ext)
                  ? 'video'
                  : 'audio';
      if (!mounted) return;
      setState(() {
        _pendingMedia = PostMedia(
          url: 'blob://$blobHash',
          type: type,
          blobHash: blobHash,
          size: (manifest['total_size'] as num?)?.toInt() ?? 0,
        );
        _uploadingMedia = false;
      });
    } catch (e) {
      if (mounted) {
        setState(() => _uploadingMedia = false);
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: SelectableText('Attach failed: $e')),
        );
      }
    }
  }

  Future<void> _publishPost() async {
    if (_isPosting) return;
    if (_contentController.text.isEmpty) {
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(content: SelectableText('Post content cannot be empty')),
      );
      return;
    }

    final text = _contentController.text;
    if (!context.read<FeedService>().validateNote(text)) {
      if (context.mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(
              content: SelectableText('Note rejected by content validator')),
        );
      }
      return;
    }

    setState(() => _isPosting = true);
    try {
      if (await context.read<SignerService>().isLocked()) {
        if (mounted) {
          ScaffoldMessenger.of(context).showSnackBar(
            const SnackBar(
              content: SelectableText('Signer locked — unlock in Security settings'),
            ),
          );
        }
        return;
      }
    } finally {
      setState(() => _isPosting = false);
    }
    try {
      final feedService = context.read<FeedService>();
      final sessionService = context.read<SessionService>();

      if (sessionService.activePubkey == null) {
        throw Exception('No active account');
      }

      final tags = <List<String>>[
        ..._tags.map((tag) => ['t', tag]),
        ..._mentions.map((mention) => ['p', mention]),
        if (_pendingMedia != null)
          [
            'media',
            _pendingMedia!.type,
            _pendingMedia!.url,
            _pendingMedia!.blobHash,
            _pendingMedia!.size.toString(),
          ],
      ];

      await feedService.publishTextNote(
        _contentController.text,
        tags,
        sessionService.activePubkey!,
      );

      if (mounted) {
        Navigator.of(context).pop();
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(content: SelectableText('Post published!')),
        );
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: SelectableText('Error: $e')),
        );
      }
    } finally {
      setState(() => _isPosting = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: EdgeInsets.only(
        bottom: MediaQuery.of(context).viewInsets.bottom,
        left: 16,
        right: 16,
        top: 16,
      ),
      child: SingleChildScrollView(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            // Header
            Row(
              mainAxisAlignment: MainAxisAlignment.spaceBetween,
              children: [
                const Text(
                  'New Post',
                  style: TextStyle(fontSize: 20, fontWeight: FontWeight.bold),
                ),
                IconButton(
                  icon: const Icon(Icons.close),
                  onPressed: () => Navigator.of(context).pop(),
                ),
              ],
            ),
            const SizedBox(height: 16),
            // Content input
            TextField(
              controller: _contentController,
              onChanged: (_) => _onContentChanged(),
              decoration: InputDecoration(
                hintText: 'What\'s happening?',
                border: OutlineInputBorder(
                  borderRadius: BorderRadius.circular(8),
                ),
              ),
              maxLines: 5,
              enabled: !_isPosting,
            ),
            const SizedBox(height: 8),
            if (_detectedTags.isNotEmpty)
              Wrap(
                spacing: 8,
                runSpacing: 4,
                children: _detectedTags.map((tag) {
                  final already = _tags.contains(tag);
                  return ActionChip(
                    avatar: already ? const Icon(Icons.check, size: 16) : null,
                    label: Text('#$tag'),
                    onPressed:
                        already ? null : () => setState(() => _tags.add(tag)),
                  );
                }).toList(),
              ),
            const SizedBox(height: 16),
            // Tags section
            if (_tags.isNotEmpty) ...[
              const Text(
                'Tags',
                style: TextStyle(fontWeight: FontWeight.bold),
              ),
              Wrap(
                spacing: 8,
                children: _tags.map((tag) {
                  return Chip(
                    label: Text('#$tag'),
                    onDeleted: () {
                      setState(() => _tags.remove(tag));
                    },
                  );
                }).toList(),
              ),
              const SizedBox(height: 16),
            ],
            // Mentions section
            if (_mentions.isNotEmpty) ...[
              const Text(
                'Mentions',
                style: TextStyle(fontWeight: FontWeight.bold),
              ),
              Wrap(
                spacing: 8,
                children: _mentions.map((mention) {
                  return Chip(
                    label: Text('@${mention.substring(0, 8)}...'),
                    onDeleted: () {
                      setState(() => _mentions.remove(mention));
                    },
                  );
                }).toList(),
              ),
              const SizedBox(height: 16),
            ],
            // Action buttons
            Row(
              mainAxisAlignment: MainAxisAlignment.spaceEvenly,
              children: [
                IconButton(
                  icon: const Icon(Icons.tag),
                  onPressed: _isPosting ? null : _showTagDialog,
                  tooltip: 'Add tag',
                ),
                IconButton(
                  icon: const Icon(Icons.person_add),
                  onPressed: _isPosting ? null : _showMentionDialog,
                  tooltip: 'Mention user',
                ),
                IconButton(
                  icon: _uploadingMedia
                      ? const SizedBox(
                          height: 20,
                          width: 20,
                          child: CircularProgressIndicator(strokeWidth: 2),
                        )
                      : const Icon(Icons.attach_file),
                  onPressed:
                      _isPosting || _uploadingMedia ? null : _attachMedia,
                  tooltip: 'Attach media',
                ),
              ],
            ),
            if (_pendingMedia != null) ...[
              const SizedBox(height: 8),
              Chip(
                avatar: Icon(
                  _pendingMedia!.type == 'image'
                      ? Icons.image
                      : _pendingMedia!.type == 'video'
                          ? Icons.videocam
                          : Icons.audiotrack,
                ),
                label: Text(
                  '${_pendingMedia!.type}: ${_pendingMedia!.blobHash.substring(0, 10)}…',
                  overflow: TextOverflow.ellipsis,
                ),
                onDeleted: () => setState(() => _pendingMedia = null),
              ),
            ],
            const SizedBox(height: 16),
            // Publish button
            SizedBox(
              width: double.maxFinite,
              child: ElevatedButton(
                onPressed: _isPosting ? null : _publishPost,
                child: _isPosting
                    ? const SizedBox(
                        height: 20,
                        width: 20,
                        child: CircularProgressIndicator(strokeWidth: 2),
                      )
                    : const Text('Publish'),
              ),
            ),
            const SizedBox(height: 16),
          ],
        ),
      ),
    );
  }

  void _showTagDialog() {
    final controller = TextEditingController();
    showDialog(
      context: context,
      builder: (context) {
        return AlertDialog(
          title: const Text('Add Tag'),
          content: TextField(
            controller: controller,
            decoration: const InputDecoration(hintText: 'Tag name'),
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.of(context).pop(),
              child: const Text('Cancel'),
            ),
            TextButton(
              onPressed: () {
                if (controller.text.isNotEmpty) {
                  setState(() => _tags.add(controller.text));
                  Navigator.of(context).pop();
                }
              },
              child: const Text('Add'),
            ),
          ],
        );
      },
    );
  }

  void _showMentionDialog() {
    final controller = TextEditingController();
    List<SearchResultItem> results = [];
    showDialog(
      context: context,
      builder: (context) {
        return StatefulBuilder(
          builder: (context, setDialogState) {
            void onQuery(String q) async {
              final list = q.trim().isEmpty
                  ? <SearchResultItem>[]
                  : await context
                      .read<SearchService>()
                      .searchProfiles(q.trim(), limit: 8)
                      .catchError((e) => <SearchResultItem>[]);
              setDialogState(() => results = list);
            }

            return AlertDialog(
              title: const Text('Mention User'),
              content: Column(
                mainAxisSize: MainAxisSize.min,
                children: [
                  TextField(
                    controller: controller,
                    autofocus: true,
                    onChanged: onQuery,
                    decoration: InputDecoration(
                      hintText: 'Name, pubkey or npub',
                      suffixIcon: IconButton(
                        icon: const Icon(Icons.search),
                        onPressed: () => onQuery(controller.text),
                      ),
                    ),
                  ),
                  const SizedBox(height: 8),
                  if (results.isNotEmpty)
                    SizedBox(
                      height: 160,
                      child: ListView.builder(
                        shrinkWrap: true,
                        itemCount: results.length,
                        itemBuilder: (context, i) {
                          final r = results[i];
                          return ListTile(
                            dense: true,
                            leading: const Icon(Icons.person_outline),
                            title: Text(r.title,
                                maxLines: 1, overflow: TextOverflow.ellipsis),
                            subtitle: Text(
                              (r.pubkey ?? r.id).substring(0, 12),
                              style: const TextStyle(fontSize: 11),
                            ),
                            onTap: () {
                              final key = r.pubkey ?? r.id;
                              if (key.isNotEmpty) {
                                setState(() => _mentions.add(key));
                                Navigator.of(context).pop();
                              }
                            },
                          );
                        },
                      ),
                    ),
                ],
              ),
              actions: [
                TextButton(
                  onPressed: () => Navigator.of(context).pop(),
                  child: const Text('Cancel'),
                ),
                TextButton(
                  onPressed: () {
                    if (controller.text.isNotEmpty) {
                      setState(() => _mentions.add(controller.text));
                      Navigator.of(context).pop();
                    }
                  },
                  child: const Text('Add'),
                ),
              ],
            );
          },
        );
      },
    );
  }
}
