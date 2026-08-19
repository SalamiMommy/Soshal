import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';
import 'package:qr_flutter/qr_flutter.dart';
import '../services/friends_service.dart';
import '../services/media_service.dart';
import '../services/messaging_service.dart';
import '../services/p2p_service.dart';
import '../services/profile_service.dart';
import '../services/session_service.dart';
import '../services/shell_service.dart';
import '../utils/blob_resolver.dart';
import '../utils/format.dart';
import '../widgets/blob_image.dart';

/// ProfileRendererScreen. Displays custom profile widgets with optional
/// drag-and-drop layout editing mode.
class ProfileRendererScreen extends StatefulWidget {
  final String pubkey;
  final bool initialEditMode;

  const ProfileRendererScreen({
    super.key,
    required this.pubkey,
    this.initialEditMode = false,
  });

  @override
  State<ProfileRendererScreen> createState() => _ProfileRendererScreenState();
}

class _ProfileRendererScreenState extends State<ProfileRendererScreen> {
  List<CustomProfileNode> _nodes = [];
  List<CustomProfileNode> _localNodes = [];
  String _themeId = 'default';
  bool _loading = true;
  bool _editMode = false;
  bool _saving = false;

  @override
  void initState() {
    super.initState();
    _editMode = widget.initialEditMode;
    _loadProfile();
  }

  Future<void> _loadProfile() async {
    setState(() => _loading = true);
    try {
      final profileData = context
          .read<ProfileService>()
          .getCustomProfileNodes(pubkey: widget.pubkey);
      if (profileData.isNotEmpty && profileData != '[]') {
        try {
          final decoded = jsonDecode(profileData);
          final loadedNodes = decoded is List
              ? decoded
                  .map((e) =>
                      CustomProfileNode.fromJson(e as Map<String, dynamic>))
                  .toList()
              : CustomProfile.fromJson(decoded as Map<String, dynamic>).nodes;
          setState(() {
            _nodes = loadedNodes;
            _localNodes = loadedNodes;
            _themeId = decoded is Map<String, dynamic>
                ? (decoded['themeId'] as String? ?? 'default')
                : _themeId;
          });
        } catch (e) {
          debugPrint('Failed to parse profile data: $e');
        }
      }
    } catch (e) {
      debugPrint('ProfileRendererScreen: load error: $e');
      // Set default nodes if none exist
      setState(() {
        _nodes = [];
        _localNodes = [];
      });
    } finally {
      if (mounted) setState(() => _loading = false);
    }
  }

  void _toggleEditMode(bool enabled) {
    setState(() {
      _editMode = enabled;
      _localNodes = List.from(_nodes);
    });
  }

  void _handleNudgeNode(String nodeId, double dx, double dy) {
    setState(() {
      _localNodes = _localNodes.map((node) {
        if (node.id == nodeId) {
          final currentOffsetX = node.styles.offsetX ?? 0;
          final currentOffsetY = node.styles.offsetY ?? 0;
          double snap(double v) => (v / 8).roundToDouble() * 8;
          return node.copyWith(
            styles: node.styles.copyWith(
              offsetX: snap(currentOffsetX + dx),
              offsetY: snap(currentOffsetY + dy),
            ),
          );
        }
        return node;
      }).toList();
    });
  }

  void _handleResizeNodeWidth(String nodeId, String width) {
    setState(() {
      _localNodes = _localNodes.map((node) {
        if (node.id == nodeId) {
          return node.copyWith(
            styles: node.styles.copyWith(width: width),
          );
        }
        return node;
      }).toList();
    });
  }

  void _handleDeleteWidget(String nodeId) {
    setState(
        () => _localNodes = _localNodes.where((n) => n.id != nodeId).toList());
  }

  Future<void> _handleSaveReorder() async {
    setState(() => _saving = true);
    try {
      final profile = CustomProfile(themeId: _themeId, nodes: _localNodes);
      context.read<ProfileService>().saveCustomProfile(
            pubkey: widget.pubkey,
            profileJson: jsonEncode(profile.toJson()),
          );

      setState(() {
        _nodes = _localNodes;
        _editMode = false;
      });

      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(content: Text('Layout saved')),
        );
      }
    } catch (e) {
      debugPrint('ProfileRendererScreen: save error: $e');
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text('Failed to save: $e')),
        );
      }
    } finally {
      if (mounted) setState(() => _saving = false);
    }
  }

  void _showQrCode() {
    final npub =
        context.read<ProfileService>().npubEncode(publicKey: widget.pubkey);
    showDialog<void>(
      context: context,
      builder: (dialogContext) => AlertDialog(
        title: const Text('Profile QR'),
        content: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            QrImageView(data: 'nostr:$npub', size: 220),
            const SizedBox(height: 16),
            SelectableText(npub, textAlign: TextAlign.center),
          ],
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(dialogContext).pop(),
            child: const Text('Close'),
          ),
        ],
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final sessionService = context.read<SessionService>();
    final isMine = widget.pubkey == sessionService.activePubkey;

    if (_loading) {
      return Scaffold(
        appBar: AppBar(title: const Text('Profile')),
        body: const Center(child: CircularProgressIndicator()),
      );
    }

    if (_editMode && isMine) {
      return Scaffold(
        appBar: AppBar(
          title: const Text('Layout Editor'),
          elevation: 0,
          actions: [
            TextButton(
              onPressed: () => _toggleEditMode(false),
              child: const Text('Cancel'),
            ),
            TextButton(
              onPressed: _saving ? null : _handleSaveReorder,
              child: _saving
                  ? const SizedBox(
                      width: 16,
                      height: 16,
                      child: CircularProgressIndicator(strokeWidth: 2),
                    )
                  : const Text('Done'),
            ),
          ],
        ),
        body: Column(
          children: [
            Container(
              padding: const EdgeInsets.all(12),
              margin: const EdgeInsets.all(16),
              decoration: BoxDecoration(
                color: theme.colorScheme.surface,
                borderRadius: BorderRadius.circular(8),
                border: Border.all(color: theme.colorScheme.primary),
              ),
              child: Text(
                '⠿ Drag widget body to move (8px snap) | ▲▼◀▶ Nudge | ↔ Resize',
                style: theme.textTheme.bodySmall?.copyWith(
                  color: theme.colorScheme.primary,
                  fontWeight: FontWeight.bold,
                ),
                textAlign: TextAlign.center,
              ),
            ),
            Expanded(
              child: _localNodes.isEmpty
                  ? Center(
                      child: Text(
                        'No widgets. Go back and use "Build My Profile" to add some.',
                        style: theme.textTheme.bodyMedium?.copyWith(
                          color: theme.colorScheme.onSurface
                              .withValues(alpha: 0.6),
                        ),
                        textAlign: TextAlign.center,
                      ),
                    )
                  : ListView.builder(
                      itemCount: _localNodes.length,
                      itemBuilder: (context, index) {
                        final node = _localNodes[index];
                        return _EditableWidgetCard(
                          key: ValueKey(node.id),
                          node: node,
                          pubkey: widget.pubkey,
                          onNudge: _handleNudgeNode,
                          onResize: _handleResizeNodeWidth,
                          onDelete: _handleDeleteWidget,
                        );
                      },
                    ),
            ),
          ],
        ),
      );
    }

    return Scaffold(
      appBar: AppBar(
        title: const Text('Profile'),
        elevation: 0,
        actions: [
          IconButton(
            icon: const Icon(Icons.qr_code),
            onPressed: _showQrCode,
          ),
          if (isMine)
            _editMode
                ? Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      TextButton(
                        onPressed: () => _toggleEditMode(false),
                        child: const Text('Cancel'),
                      ),
                      TextButton(
                        onPressed: _saving ? null : _handleSaveReorder,
                        child: const Text('Done'),
                      ),
                    ],
                  )
                : TextButton.icon(
                    onPressed: () => _toggleEditMode(true),
                    icon: const Text('🎨'),
                    label: const Text('Edit Layout'),
                  ),
        ],
      ),
      body: SingleChildScrollView(
        child: Column(
          children: [
            const SizedBox(height: 16),
            if (_nodes.isEmpty) ...[
              const DefaultPostHistoryWidget(),
              _GuestbookWidget(pubkey: widget.pubkey),
              if (isMine)
                Padding(
                  padding: const EdgeInsets.all(16.0),
                  child: ElevatedButton(
                    onPressed: () => context.push('/profile-builder'),
                    child: const Text('Build My Profile'),
                  ),
                ),
            ] else ...[
              for (final node in _nodes)
                _WidgetDisplayCard(node: node, pubkey: widget.pubkey),
              if (!_nodes.any((n) => n.type == 'post_history'))
                const DefaultPostHistoryWidget(),
              if (!_nodes.any((n) => n.type == 'guestbook'))
                _GuestbookWidget(pubkey: widget.pubkey),
            ],
          ],
        ),
      ),
    );
  }
}

class _EditableWidgetCard extends StatelessWidget {
  final CustomProfileNode node;
  final String pubkey;
  final Function(String, double, double) onNudge;
  final Function(String, String) onResize;
  final Function(String) onDelete;

  const _EditableWidgetCard({
    super.key,
    required this.node,
    required this.pubkey,
    required this.onNudge,
    required this.onResize,
    required this.onDelete,
  });

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final currentWidth = node.styles.width ?? '100%';

    return Card(
      margin: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
      child: Column(
        children: [
          Container(
            padding: const EdgeInsets.all(8),
            decoration: BoxDecoration(
              color: theme.colorScheme.surface,
              border: Border.all(color: theme.colorScheme.outline),
              borderRadius:
                  const BorderRadius.vertical(top: Radius.circular(12)),
            ),
            child: Row(
              children: [
                Text(
                  '⠿',
                  style: TextStyle(
                    fontSize: 20,
                    color: theme.colorScheme.onSurface.withValues(alpha: 0.4),
                  ),
                ),
                const SizedBox(width: 8),
                // Nudge buttons
                Row(
                  children: [
                    _NudgeButton(
                      icon: Icons.arrow_upward,
                      onPressed: () => onNudge(node.id, 0, -10),
                    ),
                    _NudgeButton(
                      icon: Icons.arrow_downward,
                      onPressed: () => onNudge(node.id, 0, 10),
                    ),
                    _NudgeButton(
                      icon: Icons.arrow_back,
                      onPressed: () => onNudge(node.id, -10, 0),
                    ),
                    _NudgeButton(
                      icon: Icons.arrow_forward,
                      onPressed: () => onNudge(node.id, 10, 0),
                    ),
                  ],
                ),
                const SizedBox(width: 8),
                // Width resize buttons
                Row(
                  children: [
                    _WidthButton(
                      label: '50%',
                      isSelected: currentWidth == '50%',
                      onPressed: () => onResize(node.id, '50%'),
                    ),
                    _WidthButton(
                      label: '75%',
                      isSelected: currentWidth == '75%',
                      onPressed: () => onResize(node.id, '75%'),
                    ),
                    _WidthButton(
                      label: '100%',
                      isSelected:
                          currentWidth == '100%' || currentWidth == null,
                      onPressed: () => onResize(node.id, '100%'),
                    ),
                  ],
                ),
                const Spacer(),
                IconButton(
                  icon: const Icon(Icons.close),
                  color: theme.colorScheme.error,
                  onPressed: () => onDelete(node.id),
                ),
              ],
            ),
          ),
          Padding(
            padding: const EdgeInsets.all(16),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                GestureDetector(
                  onPanUpdate: (d) => onNudge(node.id, d.delta.dx, d.delta.dy),
                  child: _WidgetRenderer(node: node, pubkey: pubkey),
                ),
                const SizedBox(height: 8),
                Text(
                  'offset: (${(node.styles.offsetX ?? 0).round()}, '
                  '${(node.styles.offsetY ?? 0).round()})',
                  style: theme.textTheme.bodySmall?.copyWith(
                    color: theme.colorScheme.onSurface.withValues(alpha: 0.4),
                  ),
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

class _NudgeButton extends StatelessWidget {
  final IconData icon;
  final VoidCallback onPressed;

  const _NudgeButton({
    required this.icon,
    required this.onPressed,
  });

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Container(
      margin: const EdgeInsets.symmetric(horizontal: 2),
      child: IconButton(
        icon: Icon(icon, size: 16),
        onPressed: onPressed,
        padding: const EdgeInsets.all(4) as EdgeInsets?,
        constraints: const BoxConstraints(minWidth: 32, minHeight: 32),
        style: IconButton.styleFrom(
          backgroundColor: theme.colorScheme.surface,
          foregroundColor: theme.colorScheme.onSurface,
        ),
      ),
    );
  }
}

class _WidthButton extends StatelessWidget {
  final String label;
  final bool isSelected;
  final VoidCallback onPressed;

  const _WidthButton({
    required this.label,
    required this.isSelected,
    required this.onPressed,
  });

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Container(
      margin: const EdgeInsets.symmetric(horizontal: 2),
      child: OutlinedButton(
        onPressed: onPressed,
        style: OutlinedButton.styleFrom(
          backgroundColor: isSelected
              ? theme.colorScheme.primary
              : theme.colorScheme.surface,
          foregroundColor: isSelected
              ? theme.colorScheme.onPrimary
              : theme.colorScheme.onSurface,
          padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
          minimumSize: const Size(40, 32),
        ),
        child: Text(
          label,
          style: const TextStyle(fontSize: 10, fontWeight: FontWeight.bold),
        ),
      ),
    );
  }
}

class _WidgetDisplayCard extends StatelessWidget {
  final CustomProfileNode node;
  final String pubkey;

  const _WidgetDisplayCard({required this.node, required this.pubkey});

  @override
  Widget build(BuildContext context) {
    final offsetX = node.styles.offsetX ?? 0;
    final offsetY = node.styles.offsetY ?? 0;
    final width = node.styles.width;

    Widget body = Padding(
      padding: const EdgeInsets.all(16),
      child: _WidgetRenderer(node: node, pubkey: pubkey),
    );

    if (width is String && width.endsWith('%')) {
      final factor =
          double.tryParse(width.replaceFirst('%', ''))?.clamp(0.1, 1.0);
      if (factor != null) {
        body = FractionallySizedBox(
          alignment: Alignment.centerLeft,
          widthFactor: factor,
          child: body,
        );
      }
    }

    return Card(
      margin: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
      child: (offsetX != 0 || offsetY != 0)
          ? Transform.translate(offset: Offset(offsetX, offsetY), child: body)
          : body,
    );
  }
}

class _WidgetRenderer extends StatelessWidget {
  final CustomProfileNode node;
  final String pubkey;

  const _WidgetRenderer({required this.node, required this.pubkey});

  @override
  Widget build(BuildContext context) {
    final allTypes = context.read<ProfileService>().nodeTypes;
    final typeInfo = allTypes.firstWhere(
      (t) => t.type == node.type,
      orElse: () => allTypes[0],
    );

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Row(
          children: [
            Text(typeInfo.icon, style: const TextStyle(fontSize: 24)),
            const SizedBox(width: 8),
            Text(
              node.properties['title'] as String? ?? typeInfo.label,
              style: Theme.of(context).textTheme.titleMedium,
            ),
          ],
        ),
        const SizedBox(height: 8),
        _renderWidgetContent(context, node),
      ],
    );
  }

  Widget _renderWidgetContent(BuildContext context, CustomProfileNode node) {
    switch (node.type) {
      case 'text_block':
        final props = TextBlockProperties.fromJson(node.properties);
        return Text(props.content);
      case 'media_gallery':
        final props = MediaGalleryProperties.fromJson(node.properties);
        if (props.items.isEmpty) {
          return const Text('No media items');
        }
        return GridView.builder(
          shrinkWrap: true,
          physics: const NeverScrollableScrollPhysics(),
          gridDelegate: SliverGridDelegateWithFixedCrossAxisCount(
            crossAxisCount: props.columns ?? 3,
            crossAxisSpacing: 4,
            mainAxisSpacing: 4,
          ),
          itemCount: props.items.length,
          itemBuilder: (context, index) {
            final item = props.items[index];
            return Image.network(item.url, fit: BoxFit.cover, cacheWidth: 256);
          },
        );
      case 'friend_grid':
        final props = FriendGridProperties.fromJson(node.properties);
        return _FriendGridWidget(pubkey: pubkey, limit: props.limit);
      case 'music_player':
        final props = MusicPlayerProperties.fromJson(node.properties);
        if (props.tracks.isEmpty) {
          return const Text('No tracks');
        }
        return _MusicPlayerWidget(tracks: props.tracks);
      case 'contact_card':
        final props = ContactCardProperties.fromJson(node.properties);
        return Wrap(
          spacing: 8,
          children: [
            if (props.enableMessage) const Chip(label: Text('Message')),
            if (props.enableVouch) const Chip(label: Text('Vouch')),
            if (props.enableAddFriend) const Chip(label: Text('Add Friend')),
          ],
        );
      case 'qa_list':
        final props = QAListProperties.fromJson(node.properties);
        if (props.pairs.isEmpty) {
          return const Text('No Q&A pairs');
        }
        return Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: props.pairs
              .map((pair) => Padding(
                    padding: const EdgeInsets.symmetric(vertical: 4),
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text(pair.question,
                            style:
                                const TextStyle(fontWeight: FontWeight.bold)),
                        Text(pair.answer),
                      ],
                    ),
                  ))
              .toList(),
        );
      case 'guestbook':
        return _GuestbookWidget(pubkey: pubkey);
      case 'profile_links':
        final props = ProfileLinksProperties.fromJson(node.properties);
        return Wrap(
          spacing: 8,
          children: [
            if (props.showMinis) const Chip(label: Text('Minis')),
            if (props.showMusicloud) const Chip(label: Text('Musicloud')),
          ],
        );
      case 'post_history':
        return const DefaultPostHistoryWidget();
      case 'theme':
        final props = ThemeProperties.fromJson(node.properties);
        return Text('Theme: ${props.themeName}');
      case 'container':
      case 'tab_container':
      default:
        return const Text('Widget content');
    }
  }
}

class DefaultPostHistoryWidget extends StatelessWidget {
  const DefaultPostHistoryWidget({super.key});

  @override
  Widget build(BuildContext context) {
    return Card(
      margin: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                const Text('📜', style: TextStyle(fontSize: 24)),
                const SizedBox(width: 8),
                Text(
                  'Post History',
                  style: Theme.of(context).textTheme.titleMedium,
                ),
              ],
            ),
            const SizedBox(height: 8),
            const Text('Recent posts will appear here'),
          ],
        ),
      ),
    );
  }
}

class _GuestbookWidget extends StatefulWidget {
  final String pubkey;

  const _GuestbookWidget({required this.pubkey});

  @override
  State<_GuestbookWidget> createState() => _GuestbookWidgetState();
}

class _GuestbookWidgetState extends State<_GuestbookWidget> {
  List<GuestbookEntry> _entries = [];
  bool _loading = true;
  bool _signing = false;
  final TextEditingController _content = TextEditingController();

  bool get _isMine =>
      context.read<SessionService>().activePubkey == widget.pubkey;

  @override
  void initState() {
    super.initState();
    _load();
  }

  @override
  void dispose() {
    _content.dispose();
    super.dispose();
  }

  Future<void> _load() async {
    setState(() => _loading = true);
    try {
      final json = context.read<ProfileService>().guestbookList(
            profilePubkey: widget.pubkey,
            limit: 50,
            onlyApproved: !_isMine,
          );
      final list = (jsonDecode(json) as List<dynamic>)
          .map((e) => GuestbookEntry.fromJson(e as Map<String, dynamic>))
          .toList();
      if (mounted) setState(() => _entries = list);
    } catch (e) {
      debugPrint('guestbook load: $e');
    } finally {
      if (mounted) setState(() => _loading = false);
    }
  }

  Future<void> _sign() async {
    final content = _content.text.trim();
    if (content.isEmpty) return;
    setState(() => _signing = true);
    try {
      await context.read<ProfileService>().guestbookAdd(
            profilePubkey: widget.pubkey,
            content: content,
          );
      _content.clear();
      await _load();
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(content: Text('Guestbook signed')),
        );
      }
    } catch (e) {
      debugPrint('guestbook sign: $e');
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text('Sign failed: $e')),
        );
      }
    } finally {
      if (mounted) setState(() => _signing = false);
    }
  }

  Future<void> _approve(GuestbookEntry entry, bool approved) async {
    try {
      await context
          .read<ProfileService>()
          .guestbookApprove(entryId: entry.id, approved: approved);
      await _load();
    } catch (e) {
      debugPrint('guestbook approve: $e');
    }
  }

  Future<void> _delete(GuestbookEntry entry) async {
    try {
      context.read<ProfileService>().guestbookDelete(entryId: entry.id);
      await _load();
    } catch (e) {
      debugPrint('guestbook delete: $e');
    }
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Card(
      margin: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                const Text('📖', style: TextStyle(fontSize: 24)),
                const SizedBox(width: 8),
                Text('Guestbook', style: theme.textTheme.titleMedium),
              ],
            ),
            const SizedBox(height: 8),
            if (_loading)
              const Padding(
                padding: EdgeInsets.all(8),
                child: Center(child: CircularProgressIndicator(strokeWidth: 2)),
              )
            else if (_entries.isEmpty)
              Padding(
                padding: const EdgeInsets.symmetric(vertical: 8),
                child: Text(
                  'No entries yet — sign it!',
                  style: theme.textTheme.bodyMedium?.copyWith(
                    color: theme.colorScheme.onSurface.withValues(alpha: 0.6),
                  ),
                ),
              )
            else
              for (final entry in _entries)
                Padding(
                  padding: const EdgeInsets.symmetric(vertical: 4),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Row(
                        children: [
                          Expanded(
                            child: Text(
                              entry.name.isNotEmpty
                                  ? entry.name
                                  : entry.pubkey.substring(0, 12),
                              style:
                                  const TextStyle(fontWeight: FontWeight.bold),
                            ),
                          ),
                          Text(
                            relativeTime(entry.createdAt),
                            style: theme.textTheme.bodySmall?.copyWith(
                              color: theme.colorScheme.onSurface
                                  .withValues(alpha: 0.4),
                            ),
                          ),
                          if (_isMine) ...[
                            IconButton(
                              icon: const Icon(Icons.check, size: 18),
                              color: entry.approved == true
                                  ? Colors.green
                                  : theme.colorScheme.outline,
                              tooltip: 'Approve',
                              onPressed: () =>
                                  _approve(entry, !(entry.approved == true)),
                            ),
                            IconButton(
                              icon: const Icon(Icons.close, size: 18),
                              color: theme.colorScheme.error,
                              tooltip: 'Reject',
                              onPressed: () => _approve(entry, false),
                            ),
                            IconButton(
                              icon: const Icon(Icons.delete, size: 18),
                              color: theme.colorScheme.error,
                              tooltip: 'Delete',
                              onPressed: () => _delete(entry),
                            ),
                          ],
                        ],
                      ),
                      Text(entry.content),
                    ],
                  ),
                ),
            const SizedBox(height: 8),
            if (!_isMine)
              Row(
                children: [
                  Expanded(
                    child: TextField(
                      controller: _content,
                      maxLength: 2000,
                      decoration: const InputDecoration(
                        hintText: 'Sign the guestbook…',
                        isDense: true,
                        counterText: '',
                      ),
                    ),
                  ),
                  const SizedBox(width: 8),
                  _signing
                      ? const SizedBox(
                          width: 20,
                          height: 20,
                          child: CircularProgressIndicator(strokeWidth: 2),
                        )
                      : ElevatedButton(
                          onPressed: _sign,
                          child: const Text('Sign'),
                        ),
                ],
              ),
          ],
        ),
      ),
    );
  }
}

class _FriendGridWidget extends StatefulWidget {
  final String pubkey;
  final int limit;

  const _FriendGridWidget({required this.pubkey, required this.limit});

  @override
  State<_FriendGridWidget> createState() => _FriendGridWidgetState();
}

class _FriendGridWidgetState extends State<_FriendGridWidget> {
  List<ProfileInfo> _friends = [];
  bool _loading = true;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    setState(() => _loading = true);
    final identity = context.read<IdentityService>();
    final friends = context.read<FriendsService>();
    try {
      final json = await friends.fetchFollows(widget.pubkey);
      final pubkeys = (jsonDecode(json) as List<dynamic>)
          .map((e) => e as String)
          .take(widget.limit)
          .toList();
      final profiles = <ProfileInfo>[];
      for (final pk in pubkeys) {
        try {
          profiles.add(await identity.getProfile(pk));
        } catch (_) {}
      }
      if (mounted) setState(() => _friends = profiles);
    } catch (e) {
      debugPrint('friend grid load: $e');
    } finally {
      if (mounted) setState(() => _loading = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    if (_loading) {
      return const Padding(
        padding: EdgeInsets.all(8),
        child: Center(child: CircularProgressIndicator(strokeWidth: 2)),
      );
    }
    if (_friends.isEmpty) {
      return const Text('No friends yet');
    }
    return Wrap(
      spacing: 12,
      runSpacing: 8,
      children: [
        for (final friend in _friends)
          Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              ClipOval(
                child: friend.picture.isNotEmpty
                    ? BlobImage(source: friend.picture, width: 48, height: 48)
                    : CircleAvatar(
                        radius: 24,
                        child: Text(friend.name.isNotEmpty
                            ? friend.name[0].toUpperCase()
                            : '?'),
                      ),
              ),
              const SizedBox(height: 2),
              Text(
                friend.name.isNotEmpty ? friend.name : 'friend',
                style: Theme.of(context).textTheme.bodySmall,
                overflow: TextOverflow.ellipsis,
              ),
            ],
          ),
      ],
    );
  }
}

class _MusicPlayerWidget extends StatelessWidget {
  final List<AudioTrack> tracks;

  const _MusicPlayerWidget({required this.tracks});

  Future<void> _play(BuildContext context, AudioTrack track) async {
    final shell = context.read<ShellService>();
    var url = track.url;
    if (url.startsWith('blob:')) {
      final hash = url.substring(5);
      final media = context.read<MediaService>();
      final p2p = context.read<P2pService>();
      final path = await resolveBlobPath(media, p2p, hash);
      if (path == null) {
        if (context.mounted) {
          ScaffoldMessenger.of(context).showSnackBar(
            const SnackBar(
                content: Text('Audio unavailable — no device has this blob.')),
          );
        }
        return;
      }
      await media.startLocalServer();
      url = media.getLocalUrl(hash);
    }
    await shell.playAudio(url, track.title);
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        for (final track in tracks)
          ListTile(
            dense: true,
            contentPadding: EdgeInsets.zero,
            leading: const Icon(Icons.music_note),
            title: Text(track.title,
                style: theme.textTheme.bodyMedium,
                overflow: TextOverflow.ellipsis),
            subtitle: Text(track.artist,
                style: theme.textTheme.bodySmall,
                overflow: TextOverflow.ellipsis),
            trailing: IconButton(
              icon: const Icon(Icons.play_circle_outline),
              tooltip: 'Play',
              onPressed: () => _play(context, track),
            ),
          ),
      ],
    );
  }
}
