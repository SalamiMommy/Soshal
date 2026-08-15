// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';
import '../ffi/db.dart' as ffi_db;
import '../models/custom_profile.dart';
import '../models/widget.dart';
import '../services/messaging_service.dart';
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
  bool _loading = true;
  bool _saving = false;

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
        final profileData = ffi_db.dbGetCustomProfileNodes(pubkey: pubkey);
        if (profileData.isNotEmpty) {
          try {
            final List<dynamic> nodesList =
                jsonDecode(profileData) as List<dynamic>;
            setState(() {
              _nodes = nodesList
                  .map((e) =>
                      CustomProfileNode.fromJson(e as Map<String, dynamic>))
                  .toList();
            });
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
            const SnackBar(content: Text('No user identity found.')),
          );
        }
        return;
      }

      // Save to database
      final profile = CustomProfile(themeId: 'default', nodes: _nodes);
      ffi_db.dbSaveCustomProfile(
        pubkey: pubkey,
        profileJson: jsonEncode(profile.toJson()),
      );

      // Publish to relays (Nostr event kind 30085)
      try {
        await messagingService.publishCustomProfile(
          pubkey,
          jsonEncode(profile.toJson()),
        );
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
          const SnackBar(content: Text('Your profile has been updated.')),
        );
        context.pop();
      }
    } catch (e) {
      debugPrint('ProfileBuilderScreen: save error: $e');
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text('Failed to save profile: $e')),
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
            for (final typeInfo in nodeTypes)
              ListTile(
                leading: Text(typeInfo.icon),
                title: Text(typeInfo.label),
                onTap: () {
                  Navigator.of(sheetContext).pop();
                  setState(() {
                    _nodes = [
                      ..._nodes,
                      makeDefaultNode(typeInfo.type, _nodes.length)
                    ];
                  });
                },
              ),
          ],
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
              'Long-press the handle to reorder. Tap a widget to edit its properties.',
              style: theme.textTheme.bodySmall?.copyWith(
                color: theme.colorScheme.onSurface.withValues(alpha: 0.6),
              ),
            ),
          ),
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
                          final typeInfo = nodeTypes.firstWhere(
                            (t) => t.type == node.type,
                            orElse: () => nodeTypes[0],
                          );
                          return _WidgetListItem(
                            key: ValueKey(node.id),
                            node: node,
                            typeInfo: typeInfo,
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
  final Function(String) onDelete;

  const _WidgetListItem({
    super.key,
    required this.node,
    required this.typeInfo,
    required this.onDelete,
  });

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final props = node.properties;
    final title = props['title'] as String? ?? typeInfo.label;

    return Card(
      margin: const EdgeInsets.symmetric(horizontal: 16, vertical: 4),
      child: ListTile(
        leading: ReorderableDragStartListener(
          index: 0,
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
    );
  }
}
