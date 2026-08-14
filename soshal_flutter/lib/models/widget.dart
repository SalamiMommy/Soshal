import 'custom_profile.dart';

class NodeTypeInfo {
  final String type;
  final String label;
  final String icon;

  NodeTypeInfo({
    required this.type,
    required this.label,
    required this.icon,
  });
}

final List<NodeTypeInfo> nodeTypes = [
  NodeTypeInfo(type: 'theme', label: 'Theme', icon: '🎨'),
  NodeTypeInfo(type: 'container', label: 'Container', icon: '📦'),
  NodeTypeInfo(type: 'text_block', label: 'Text Block', icon: '📝'),
  NodeTypeInfo(type: 'media_gallery', label: 'Media Gallery', icon: '🖼'),
  NodeTypeInfo(type: 'friend_grid', label: 'Friend Grid', icon: '👥'),
  NodeTypeInfo(type: 'music_player', label: 'Music Player', icon: '🎵'),
  NodeTypeInfo(type: 'contact_card', label: 'Contact Card', icon: '📇'),
  NodeTypeInfo(type: 'qa_list', label: 'Q&A List', icon: '❓'),
  NodeTypeInfo(type: 'tab_container', label: 'Tab Container', icon: '📑'),
  NodeTypeInfo(type: 'guestbook', label: 'Guestbook', icon: '📖'),
  NodeTypeInfo(type: 'profile_links', label: 'Profile Links', icon: '🔗'),
  NodeTypeInfo(type: 'post_history', label: 'Post History', icon: '📜'),
];

CustomProfileNode makeDefaultNode(String type, int index) {
  final timestamp = DateTime.now().millisecondsSinceEpoch;
  final randomSuffix = timestamp % 10000;
  final id = 'widget_$timestamp${randomSuffix.toString().padLeft(4, '0')}';
  final pos = NodePosition(row: 0, column: 0, order: index);

  switch (type) {
    case 'theme':
      return CustomProfileNode(
        id: id,
        type: 'theme',
        styles: SanitizedStyles(),
        position: pos,
        properties: ThemeProperties(
          themeName: 'default',
          title: 'Theme',
        ).toJson(),
      );
    case 'container':
      return CustomProfileNode(
        id: id,
        type: 'container',
        styles: SanitizedStyles(),
        position: pos,
        properties: BaseWidgetProperties(
          title: 'Section',
        ).toJson(),
      );
    case 'text_block':
      return CustomProfileNode(
        id: id,
        type: 'text_block',
        styles: SanitizedStyles(),
        position: pos,
        properties: TextBlockProperties(
          content: '',
          title: 'About Me',
        ).toJson(),
      );
    case 'media_gallery':
      return CustomProfileNode(
        id: id,
        type: 'media_gallery',
        styles: SanitizedStyles(),
        position: pos,
        properties: MediaGalleryProperties(
          items: [],
          layoutType: 'grid',
          columns: 3,
          title: 'Gallery',
        ).toJson(),
      );
    case 'friend_grid':
      return CustomProfileNode(
        id: id,
        type: 'friend_grid',
        styles: SanitizedStyles(),
        position: pos,
        properties: FriendGridProperties(
          limit: 8,
          showOnlineStatus: true,
          title: 'Top Friends',
        ).toJson(),
      );
    case 'music_player':
      return CustomProfileNode(
        id: id,
        type: 'music_player',
        styles: SanitizedStyles(),
        position: pos,
        properties: MusicPlayerProperties(
          tracks: [],
          autoplay: false,
          loop: false,
          title: 'My Music',
        ).toJson(),
      );
    case 'contact_card':
      return CustomProfileNode(
        id: id,
        type: 'contact_card',
        styles: SanitizedStyles(),
        position: pos,
        properties: ContactCardProperties(
          enableMessage: true,
          enableVouch: false,
          enableAddFriend: true,
          title: 'Contact',
        ).toJson(),
      );
    case 'qa_list':
      return CustomProfileNode(
        id: id,
        type: 'qa_list',
        styles: SanitizedStyles(),
        position: pos,
        properties: QAListProperties(
          pairs: [],
          title: 'Q&A',
        ).toJson(),
      );
    case 'tab_container':
      return CustomProfileNode(
        id: id,
        type: 'tab_container',
        styles: SanitizedStyles(),
        position: pos,
        properties: TabContainerProperties(
          tabs: [
            TabDef(
              id: 'tab_$timestamp',
              label: 'Photos',
              items: [],
            ),
          ],
          title: 'Media Tabs',
        ).toJson(),
      );
    case 'profile_links':
      return CustomProfileNode(
        id: id,
        type: 'profile_links',
        styles: SanitizedStyles(),
        position: pos,
        properties: ProfileLinksProperties(
          showMinis: true,
          showMusicloud: true,
          title: 'Profile Links',
        ).toJson(),
      );
    case 'guestbook':
      return CustomProfileNode(
        id: id,
        type: 'guestbook',
        styles: SanitizedStyles(),
        position: pos,
        properties: GuestbookProperties(
          entries: [],
          allowAnonymous: false,
          maxEntries: 20,
          title: 'Guestbook',
        ).toJson(),
      );
    case 'post_history':
      return CustomProfileNode(
        id: id,
        type: 'post_history',
        styles: SanitizedStyles(),
        position: pos,
        properties: HistoryProperties(
          showReposts: true,
          maxEntries: 50,
          title: 'Post History',
        ).toJson(),
      );
    default:
      return CustomProfileNode(
        id: id,
        type: 'container',
        styles: SanitizedStyles(),
        position: pos,
        properties: BaseWidgetProperties(
          title: 'Widget',
        ).toJson(),
      );
  }
}
