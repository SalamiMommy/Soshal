// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';
import '../services/messaging_service.dart';
import '../services/music_service.dart';
import '../services/profile_service.dart';
import '../services/session_service.dart';

/// ProfileBuilderScreen. Drag-and-drop custom profile builder. Add, remove,
/// reorder widgets (text, media, music, friend grid, theme).
class ProfileBuilderScreen extends StatefulWidget {
  const ProfileBuilderScreen({super.key});

  @override
  State<ProfileBuilderScreen> createState() => _ProfileBuilderScreenState();
}

class _ProfileBuilderScreenState extends State<ProfileBuilderScreen> {
  List<CustomProfileNode> _nodes = [];
  String _themeId = 'default';
  bool _loading = true;
  bool _saving = false;

  static const _themes = [
    ('default', 'Default'),
    ('midnight', 'Midnight'),
    ('sunset', 'Sunset'),
    ('forest', 'Forest'),
    ('neon', 'Neon'),
    ('pastel', 'Pastel'),
  ];

  @override
  void initState() {
    super.initState();
    _loadNodes();
  }

  Future<void> _loadNodes() async {
    setState(() => _loading = true);
    try {
      final sessionService = context.read<SessionService>();
      final pubkey = sessionService.activePubkey;
      if (pubkey != null) {
        // Load from database
        final profileData = context
            .read<ProfileService>()
            .getCustomProfileNodes(pubkey: pubkey);
        if (profileData.isNotEmpty && profileData != '[]') {
          try {
            final decoded = jsonDecode(profileData);
            if (decoded is List) {
              // Legacy shape: bare node array.
              setState(() {
                _nodes = decoded
                    .map((e) =>
                        CustomProfileNode.fromJson(e as Map<String, dynamic>))
                    .toList();
              });
            } else {
              final profile =
                  CustomProfile.fromJson(decoded as Map<String, dynamic>);
              setState(() {
                _themeId = profile.themeId;
                _nodes = profile.nodes;
              });
            }
          } catch (e) {
            debugPrint('Failed to parse profile data: $e');
          }
        }
      }
    } catch (e) {
      debugPrint('ProfileBuilderScreen: loadNodes error: $e');
    } finally {
      if (mounted) setState(() => _loading = false);
    }
  }

  void _handleDragEnd(List<CustomProfileNode> reordered) {
    setState(() => _nodes = reordered);
  }

  void _handleDeleteWidget(String nodeId) {
    showDialog(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Delete Widget'),
        content: const Text('Are you sure you want to remove this widget?'),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context),
            child: const Text('Cancel'),
          ),
          TextButton(
            onPressed: () {
              setState(
                  () => _nodes = _nodes.where((n) => n.id != nodeId).toList());
              Navigator.pop(context);
            },
            style: TextButton.styleFrom(foregroundColor: Colors.red),
            child: const Text('Delete'),
          ),
        ],
      ),
    );
  }

  Future<void> _handleSaveProfile() async {
    setState(() => _saving = true);
    try {
      final sessionService = context.read<SessionService>();
      final messagingService = context.read<IdentityService>();
      final pubkey = sessionService.activePubkey;
      if (pubkey == null) {
        if (mounted) {
          ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(content: SelectableText('No user identity found.')),
          );
        }
        return;
      }

      // Save to database (Rust validates the payload first)
      final profile = CustomProfile(themeId: _themeId, nodes: _nodes);
      final profileJson = jsonEncode(profile.toJson());
      final canonical =
          context.read<ProfileService>().validateProfile(profileJson);
      context.read<ProfileService>().saveCustomProfile(
            pubkey: pubkey,
            profileJson: canonical,
          );

      // Publish to relays (Nostr event kind 30085)
      try {
        await messagingService.publishCustomProfile(pubkey, canonical);
      } catch (e) {
        debugPrint('Failed to publish to relays: $e');
        if (mounted) {
          ScaffoldMessenger.of(context).showSnackBar(
            const SnackBar(
              content: Text('Saved locally but could not publish to relays.'),
            ),
          );
        }
      }

      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: SelectableText('Your profile has been updated.')),
        );
        context.pop();
      }
    } catch (e) {
      debugPrint('ProfileBuilderScreen: save error: $e');
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: SelectableText('Failed to save profile: $e')),
        );
      }
    } finally {
      if (mounted) setState(() => _saving = false);
    }
  }

  void _handleAddWidget() {
    showModalBottomSheet<void>(
      context: context,
      builder: (sheetContext) => SafeArea(
        child: ListView(
          shrinkWrap: true,
          children: [
            Padding(
              padding: const EdgeInsets.all(16),
              child: Text('Add Widget',
                  style: Theme.of(sheetContext).textTheme.titleLarge),
            ),
            for (final typeInfo in context.read<ProfileService>().nodeTypes)
              ListTile(
                leading: Text(typeInfo.icon),
                title: Text(typeInfo.label),
                onTap: () {
                  Navigator.of(sheetContext).pop();
                  setState(() {
                    try {
                      final nodeJson = context
                          .read<ProfileService>()
                          .defaultNode(
                              type: typeInfo.type, index: _nodes.length);
                      _nodes = [
                        ..._nodes,
                        CustomProfileNode.fromJson(
                            jsonDecode(nodeJson) as Map<String, dynamic>),
                      ];
                    } catch (e) {
                      debugPrint('Failed to create node: $e');
                    }
                  });
                },
              ),
          ],
        ),
      ),
    );
  }

  void _updateNode(CustomProfileNode updated) {
    setState(() {
      _nodes = _nodes.map((n) => n.id == updated.id ? updated : n).toList();
    });
  }

  void _handleEditWidget(CustomProfileNode node) {
    showModalBottomSheet<void>(
      context: context,
      isScrollControlled: true,
      builder: (sheetContext) => SafeArea(
        child: Padding(
          padding: EdgeInsets.only(
            bottom: MediaQuery.of(sheetContext).viewInsets.bottom,
          ),
          child: _NodePropertyEditor(
            node: node,
            onChanged: (updated) {
              _updateNode(updated);
              Navigator.of(sheetContext).pop();
            },
          ),
        ),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);

    return Scaffold(
      appBar: AppBar(
        title: const Text('Edit Profile'),
        elevation: 0,
        actions: [
          if (_saving)
            const Padding(
              padding: EdgeInsets.all(16.0),
              child: SizedBox(
                width: 20,
                height: 20,
                child: CircularProgressIndicator(strokeWidth: 2),
              ),
            )
          else
            TextButton(
              onPressed: _handleSaveProfile,
              child: const Text('Save'),
            ),
        ],
      ),
      body: Column(
        children: [
          Padding(
            padding: const EdgeInsets.all(16.0),
            child: Text(
              'Tap a widget to edit its content. Drag to reorder.',
              style: theme.textTheme.bodySmall?.copyWith(
                color: theme.colorScheme.onSurface.withValues(alpha: 0.6),
              ),
            ),
          ),
          SingleChildScrollView(
            scrollDirection: Axis.horizontal,
            padding: const EdgeInsets.symmetric(horizontal: 16),
            child: Row(
              children: [
                Text('Theme: ',
                    style: theme.textTheme.bodySmall?.copyWith(
                      color: theme.colorScheme.onSurface.withValues(alpha: 0.6),
                    )),
                for (final (id, label) in _themes)
                  Padding(
                    padding: const EdgeInsets.only(right: 8),
                    child: ChoiceChip(
                      label: Text(label),
                      selected: _themeId == id,
                      onSelected: (_) => setState(() => _themeId = id),
                    ),
                  ),
              ],
            ),
          ),
          const SizedBox(height: 12),
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: 16.0),
            child: ElevatedButton.icon(
              onPressed: () {
                final sessionService = context.read<SessionService>();
                final pubkey = sessionService.activePubkey;
                if (pubkey != null) {
                  context.push('/profile-renderer/$pubkey?editMode=true');
                }
              },
              icon: const Text('🎨'),
              label: const Text('Open Live Drag & Resize Layout Editor'),
              style: ElevatedButton.styleFrom(
                backgroundColor: theme.colorScheme.surface,
                foregroundColor: theme.colorScheme.primary,
                side: BorderSide(color: theme.colorScheme.primary),
              ),
            ),
          ),
          const SizedBox(height: 16),
          Expanded(
            child: _loading
                ? const Center(child: CircularProgressIndicator())
                : _nodes.isEmpty
                    ? Center(
                        child: Column(
                          mainAxisAlignment: MainAxisAlignment.center,
                          children: [
                            Text(
                              'Your profile is empty.',
                              style: theme.textTheme.titleMedium,
                            ),
                            const SizedBox(height: 8),
                            Text(
                              'Add widgets to customize it!',
                              style: theme.textTheme.bodyMedium?.copyWith(
                                color: theme.colorScheme.onSurface
                                    .withValues(alpha: 0.6),
                              ),
                            ),
                          ],
                        ),
                      )
                    : ReorderableListView.builder(
                        itemCount: _nodes.length,
                        onReorderItem: (oldIndex, newIndex) {
                          final reordered =
                              List<CustomProfileNode>.from(_nodes);
                          final item = reordered.removeAt(oldIndex);
                          reordered.insert(newIndex, item);
                          _handleDragEnd(reordered);
                        },
                        itemBuilder: (context, index) {
                          final node = _nodes[index];
                          final allTypes =
                              context.read<ProfileService>().nodeTypes;
                          final typeInfo = allTypes.firstWhere(
                            (t) => t.type == node.type,
                            orElse: () => allTypes[0],
                          );
                          return _WidgetListItem(
                            key: ValueKey(node.id),
                            node: node,
                            typeInfo: typeInfo,
                            index: index,
                            onTap: () => _handleEditWidget(node),
                            onDelete: _handleDeleteWidget,
                          );
                        },
                      ),
          ),
        ],
      ),
      floatingActionButton: FloatingActionButton(
        onPressed: _handleAddWidget,
        child: const Icon(Icons.add),
      ),
    );
  }
}

class _WidgetListItem extends StatelessWidget {
  final CustomProfileNode node;
  final NodeTypeInfo typeInfo;
  final int index;
  final Function(String) onDelete;
  final VoidCallback onTap;

  const _WidgetListItem({
    super.key,
    required this.node,
    required this.typeInfo,
    required this.index,
    required this.onDelete,
    required this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final props = node.properties;
    final title = props['title'] as String? ?? typeInfo.label;

    return Card(
      margin: const EdgeInsets.symmetric(horizontal: 16, vertical: 4),
      child: InkWell(
        onTap: onTap,
        child: ListTile(
          leading: ReorderableDragStartListener(
            index: index,
            child: Container(
              padding: const EdgeInsets.symmetric(horizontal: 8),
              child: Text(
                '⠿',
                style: TextStyle(
                  fontSize: 20,
                  color: theme.colorScheme.onSurface.withValues(alpha: 0.4),
                ),
              ),
            ),
          ),
          leadingAndTrailingTextStyle: theme.textTheme.bodyMedium,
          title: Row(
            children: [
              Text(
                typeInfo.icon,
                style: const TextStyle(fontSize: 24),
              ),
              const SizedBox(width: 12),
              Expanded(
                child: Text(
                  title,
                  style: theme.textTheme.titleSmall,
                ),
              ),
            ],
          ),
          subtitle: Text(typeInfo.label),
          trailing: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              IconButton(
                icon: const Icon(Icons.delete, size: 20),
                color: theme.colorScheme.error,
                onPressed: () => onDelete(node.id),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class _NodePropertyEditor extends StatefulWidget {
  final CustomProfileNode node;
  final ValueChanged<CustomProfileNode> onChanged;

  const _NodePropertyEditor({
    required this.node,
    required this.onChanged,
  });

  @override
  State<_NodePropertyEditor> createState() => _NodePropertyEditorState();
}

class _NodePropertyEditorState extends State<_NodePropertyEditor> {
  late CustomProfileNode _node;
  late TextEditingController _title;
  late TextEditingController _content;
  late TextEditingController _trackUrl;
  late TextEditingController _trackTitle;
  late TextEditingController _mediaUrl;
  late TextEditingController _mediaCaption;
  late TextEditingController _qaQuestion;
  late TextEditingController _qaAnswer;
  List<MusicTrack> _musicloud = [];
  bool _musicLoading = false;

  @override
  void initState() {
    super.initState();
    _node = widget.node;
    _title =
        TextEditingController(text: _node.properties['title'] as String? ?? '');
    _content = TextEditingController(
        text: _node.properties['content'] as String? ?? '');
    _trackUrl = TextEditingController();
    _trackTitle = TextEditingController();
    _mediaUrl = TextEditingController();
    _mediaCaption = TextEditingController();
    _qaQuestion = TextEditingController();
    _qaAnswer = TextEditingController();
  }

  @override
  void dispose() {
    _title.dispose();
    _content.dispose();
    _trackUrl.dispose();
    _trackTitle.dispose();
    _mediaUrl.dispose();
    _mediaCaption.dispose();
    _qaQuestion.dispose();
    _qaAnswer.dispose();
    super.dispose();
  }

  Map<String, dynamic> get _props =>
      Map<String, dynamic>.from(_node.properties);

  void _save() {
    widget.onChanged(_node);
  }

  Future<void> _loadMusicloud() async {
    setState(() => _musicLoading = true);
    try {
      final tracks = await context.read<MusicService>().fetchTracks(limit: 100);
      if (mounted) setState(() => _musicloud = tracks);
    } catch (e) {
      debugPrint('musicloud fetch: $e');
    } finally {
      if (mounted) setState(() => _musicLoading = false);
    }
  }

  void _addMusicloudTrack(MusicTrack t) {
    final tracks =
        List<AudioTrack>.from(MusicPlayerProperties.fromJson(_props).tracks);
    tracks.add(AudioTrack(
      id: t.id,
      title: t.title,
      artist: t.pubkey.substring(0, 12),
      url: t.blobHash.isNotEmpty ? 'blob:${t.blobHash}' : t.audioUrl,
      durationSeconds: null,
    ));
    _props['tracks'] = tracks.map((e) => e.toJson()).toList();
    setState(() => _node = _node.copyWith(properties: _props));
  }

  void _addManualTrack() {
    final url = _trackUrl.text.trim();
    final title = _trackTitle.text.trim();
    if (url.isEmpty || title.isEmpty) return;
    final tracks =
        List<AudioTrack>.from(MusicPlayerProperties.fromJson(_props).tracks);
    tracks.add(AudioTrack(
      id: 't${DateTime.now().millisecondsSinceEpoch}',
      title: title,
      artist: 'you',
      url: url,
    ));
    _props['tracks'] = tracks.map((e) => e.toJson()).toList();
    _trackUrl.clear();
    _trackTitle.clear();
    setState(() => _node = _node.copyWith(properties: _props));
  }

  void _removeTrack(String id) {
    final tracks =
        List<AudioTrack>.from(MusicPlayerProperties.fromJson(_props).tracks)
          ..removeWhere((t) => t.id == id);
    _props['tracks'] = tracks.map((e) => e.toJson()).toList();
    setState(() => _node = _node.copyWith(properties: _props));
  }

  void _addMediaItem() {
    final url = _mediaUrl.text.trim();
    if (url.isEmpty) return;
    final items = List<ProfileMediaItem>.from(
        MediaGalleryProperties.fromJson(_props).items);
    items.add(ProfileMediaItem(
      id: 'm${DateTime.now().millisecondsSinceEpoch}',
      url: url,
      type: 'image',
      caption:
          _mediaCaption.text.trim().isEmpty ? null : _mediaCaption.text.trim(),
    ));
    _props['items'] = items.map((e) => e.toJson()).toList();
    _mediaUrl.clear();
    _mediaCaption.clear();
    setState(() => _node = _node.copyWith(properties: _props));
  }

  void _removeMediaItem(String id) {
    final items = List<ProfileMediaItem>.from(
        MediaGalleryProperties.fromJson(_props).items)
      ..removeWhere((i) => i.id == id);
    _props['items'] = items.map((e) => e.toJson()).toList();
    setState(() => _node = _node.copyWith(properties: _props));
  }

  void _addQaPair() {
    final q = _qaQuestion.text.trim();
    final a = _qaAnswer.text.trim();
    if (q.isEmpty || a.isEmpty) return;
    final pairs = List<QAPair>.from(QAListProperties.fromJson(_props).pairs);
    pairs.add(QAPair(question: q, answer: a));
    _props['pairs'] = pairs.map((e) => e.toJson()).toList();
    _qaQuestion.clear();
    _qaAnswer.clear();
    setState(() => _node = _node.copyWith(properties: _props));
  }

  void _removeQaPair(int index) {
    final pairs = List<QAPair>.from(QAListProperties.fromJson(_props).pairs)
      ..removeAt(index);
    _props['pairs'] = pairs.map((e) => e.toJson()).toList();
    setState(() => _node = _node.copyWith(properties: _props));
  }

  Widget _section(String label) => Padding(
        padding: const EdgeInsets.only(top: 16, bottom: 4),
        child: Text(label, style: Theme.of(context).textTheme.titleSmall),
      );

  Widget _toggle(String label, bool value, ValueChanged<bool> onChanged) =>
      SwitchListTile(
        dense: true,
        contentPadding: EdgeInsets.zero,
        title: Text(label),
        value: value,
        onChanged: onChanged,
      );

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return SingleChildScrollView(
      padding: const EdgeInsets.all(16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        mainAxisSize: MainAxisSize.min,
        children: [
          Row(
            children: [
              Text(_node.type, style: theme.textTheme.titleLarge),
              const Spacer(),
              TextButton(
                onPressed: _save,
                child: const Text('Done'),
              ),
            ],
          ),
          _section('Title'),
          TextField(
            controller: _title,
            decoration: const InputDecoration(isDense: true),
            onChanged: (v) {
              _props['title'] = v;
              setState(() => _node = _node.copyWith(properties: _props));
            },
          ),
          switch (_node.type) {
            'text_block' => Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  _section('Content'),
                  TextField(
                    controller: _content,
                    maxLines: 6,
                    onChanged: (v) {
                      _props['content'] = v;
                      setState(
                          () => _node = _node.copyWith(properties: _props));
                    },
                  ),
                  _toggle(
                    'Markdown',
                    _props['markdownEnabled'] as bool? ?? false,
                    (v) {
                      _props['markdownEnabled'] = v;
                      setState(
                          () => _node = _node.copyWith(properties: _props));
                    },
                  ),
                ],
              ),
            'media_gallery' => Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  _section('Add media item'),
                  Row(
                    children: [
                      Expanded(
                        child: TextField(
                          controller: _mediaUrl,
                          decoration: const InputDecoration(
                            hintText: 'Image URL (https)',
                            isDense: true,
                          ),
                        ),
                      ),
                      const SizedBox(width: 8),
                      Expanded(
                        child: TextField(
                          controller: _mediaCaption,
                          decoration: const InputDecoration(
                            hintText: 'Caption',
                            isDense: true,
                          ),
                        ),
                      ),
                      IconButton(
                        icon: const Icon(Icons.add),
                        onPressed: _addMediaItem,
                      ),
                    ],
                  ),
                  for (final item
                      in MediaGalleryProperties.fromJson(_props).items)
                    ListTile(
                      dense: true,
                      contentPadding: EdgeInsets.zero,
                      leading: const Icon(Icons.image),
                      title: Text(item.url,
                          overflow: TextOverflow.ellipsis,
                          style: theme.textTheme.bodySmall),
                      trailing: IconButton(
                        icon: const Icon(Icons.remove_circle_outline, size: 18),
                        color: theme.colorScheme.error,
                        onPressed: () => _removeMediaItem(item.id),
                      ),
                    ),
                ],
              ),
            'music_player' => Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  _section('From Musicloud'),
                  if (_musicloud.isEmpty)
                    OutlinedButton.icon(
                      onPressed: _musicLoading ? null : _loadMusicloud,
                      icon: _musicLoading
                          ? const SizedBox(
                              width: 14,
                              height: 14,
                              child: CircularProgressIndicator(strokeWidth: 2),
                            )
                          : const Icon(Icons.music_note),
                      label: const Text('Load my Musicloud tracks'),
                    )
                  else
                    for (final t in _musicloud.take(20))
                      ListTile(
                        dense: true,
                        contentPadding: EdgeInsets.zero,
                        title: Text(t.title,
                            overflow: TextOverflow.ellipsis,
                            style: theme.textTheme.bodySmall),
                        trailing: IconButton(
                          icon: const Icon(Icons.add_circle_outline),
                          onPressed: () => _addMusicloudTrack(t),
                        ),
                      ),
                  _section('Manual track'),
                  TextField(
                    controller: _trackTitle,
                    decoration:
                        const InputDecoration(hintText: 'Title', isDense: true),
                  ),
                  TextField(
                    controller: _trackUrl,
                    decoration: const InputDecoration(
                        hintText: 'Audio URL (https or blob:)', isDense: true),
                  ),
                  Align(
                    alignment: Alignment.centerRight,
                    child: TextButton(
                      onPressed: _addManualTrack,
                      child: const Text('Add track'),
                    ),
                  ),
                  for (final track
                      in MusicPlayerProperties.fromJson(_props).tracks)
                    ListTile(
                      dense: true,
                      contentPadding: EdgeInsets.zero,
                      leading: const Icon(Icons.music_note, size: 18),
                      title: Text('${track.title} — ${track.artist}',
                          overflow: TextOverflow.ellipsis,
                          style: theme.textTheme.bodySmall),
                      trailing: IconButton(
                        icon: const Icon(Icons.remove_circle_outline, size: 18),
                        color: theme.colorScheme.error,
                        onPressed: () => _removeTrack(track.id),
                      ),
                    ),
                ],
              ),
            'friend_grid' => Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  _section('Friends shown'),
                  Slider(
                    value: (_props['limit'] as num? ?? 8).toDouble(),
                    min: 1,
                    max: 30,
                    divisions: 29,
                    label: '${_props['limit']}',
                    onChanged: (v) {
                      _props['limit'] = v.round();
                      setState(
                          () => _node = _node.copyWith(properties: _props));
                    },
                  ),
                  _toggle(
                    'Show online status',
                    _props['showOnlineStatus'] as bool? ?? true,
                    (v) {
                      _props['showOnlineStatus'] = v;
                      setState(
                          () => _node = _node.copyWith(properties: _props));
                    },
                  ),
                ],
              ),
            'contact_card' => Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  _toggle('Message', _props['enableMessage'] as bool? ?? true,
                      (v) {
                    _props['enableMessage'] = v;
                    setState(() => _node = _node.copyWith(properties: _props));
                  }),
                  _toggle('Vouch', _props['enableVouch'] as bool? ?? false,
                      (v) {
                    _props['enableVouch'] = v;
                    setState(() => _node = _node.copyWith(properties: _props));
                  }),
                  _toggle(
                      'Add Friend', _props['enableAddFriend'] as bool? ?? true,
                      (v) {
                    _props['enableAddFriend'] = v;
                    setState(() => _node = _node.copyWith(properties: _props));
                  }),
                ],
              ),
            'qa_list' => Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  _section('Add Q&A'),
                  TextField(
                    controller: _qaQuestion,
                    decoration: const InputDecoration(
                        hintText: 'Question', isDense: true),
                  ),
                  TextField(
                    controller: _qaAnswer,
                    decoration: const InputDecoration(
                        hintText: 'Answer', isDense: true),
                  ),
                  Align(
                    alignment: Alignment.centerRight,
                    child: TextButton(
                      onPressed: _addQaPair,
                      child: const Text('Add pair'),
                    ),
                  ),
                  for (var i = 0;
                      i < QAListProperties.fromJson(_props).pairs.length;
                      i++)
                    ListTile(
                      dense: true,
                      contentPadding: EdgeInsets.zero,
                      title: Text(
                          QAListProperties.fromJson(_props).pairs[i].question,
                          style: theme.textTheme.bodySmall),
                      trailing: IconButton(
                        icon: const Icon(Icons.remove_circle_outline, size: 18),
                        color: theme.colorScheme.error,
                        onPressed: () => _removeQaPair(i),
                      ),
                    ),
                ],
              ),
            'guestbook' => Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  _toggle(
                    'Allow anonymous',
                    _props['allowAnonymous'] as bool? ?? false,
                    (v) {
                      _props['allowAnonymous'] = v;
                      setState(
                          () => _node = _node.copyWith(properties: _props));
                    },
                  ),
                  _section('Max entries (${_props['maxEntries'] ?? 20})'),
                  Slider(
                    value: (_props['maxEntries'] as num? ?? 20).toDouble(),
                    min: 5,
                    max: 100,
                    divisions: 19,
                    onChanged: (v) {
                      _props['maxEntries'] = v.round();
                      setState(
                          () => _node = _node.copyWith(properties: _props));
                    },
                  ),
                ],
              ),
            'profile_links' => Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  _toggle('Show Minis', _props['showMinis'] as bool? ?? true,
                      (v) {
                    _props['showMinis'] = v;
                    setState(() => _node = _node.copyWith(properties: _props));
                  }),
                  _toggle('Show Musicloud',
                      _props['showMusicloud'] as bool? ?? true, (v) {
                    _props['showMusicloud'] = v;
                    setState(() => _node = _node.copyWith(properties: _props));
                  }),
                ],
              ),
            _ => const SizedBox.shrink(),
          },
        ],
      ),
    );
  }
}
