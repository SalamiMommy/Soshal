/// Soshal Custom Profile Layout Node Models
/// Defines the strict, sanitized schema for MySoshal-style profile customization.
/// Allows deep aesthetic modification without exposing cross-site scripting (XSS)
/// or remote code execution (RCE).
// ignore_for_file: invalid_use_of_internal_member
library;

import 'dart:convert';

import 'package:flutter/foundation.dart';

import '../ffi/auth.dart';
import '../ffi/content.dart' as ffi_content;
import '../ffi/db.dart' as ffi_db;
import '../frb_generated.dart';
import '../utils/json_ext.dart';

const int customProfileKind = 30085;

typedef NodeType = String;

class SanitizedStyles {
  final String? backgroundColor;
  final String? borderColor;
  final String? textColor;
  final String? accentColor;
  final String? secondaryTextColor;
  final double? padding;
  final double? margin;
  final double? borderRadius;
  final double? borderWidth;
  final String? borderStyle;
  final String? flexDirection;
  final String? justifyContent;
  final String? alignItems;
  final dynamic height;
  final dynamic width;
  final String? fontFamily;
  final String? fontSize;
  final String? textAlign;
  final double? offsetX;
  final double? offsetY;

  SanitizedStyles({
    this.backgroundColor,
    this.borderColor,
    this.textColor,
    this.accentColor,
    this.secondaryTextColor,
    this.padding,
    this.margin,
    this.borderRadius,
    this.borderWidth,
    this.borderStyle,
    this.flexDirection,
    this.justifyContent,
    this.alignItems,
    this.height,
    this.width,
    this.fontFamily,
    this.fontSize,
    this.textAlign,
    this.offsetX,
    this.offsetY,
  });

  Map<String, dynamic> toJson() {
    return {
      'backgroundColor': backgroundColor,
      'borderColor': borderColor,
      'textColor': textColor,
      'accentColor': accentColor,
      'secondaryTextColor': secondaryTextColor,
      'padding': padding,
      'margin': margin,
      'borderRadius': borderRadius,
      'borderWidth': borderWidth,
      'borderStyle': borderStyle,
      'flexDirection': flexDirection,
      'justifyContent': justifyContent,
      'alignItems': alignItems,
      'height': height,
      'width': width,
      'fontFamily': fontFamily,
      'fontSize': fontSize,
      'textAlign': textAlign,
      'offsetX': offsetX,
      'offsetY': offsetY,
    };
  }

  factory SanitizedStyles.fromJson(Map<String, dynamic> json) {
    return SanitizedStyles(
      backgroundColor: json.strOrNull('backgroundColor'),
      borderColor: json.strOrNull('borderColor'),
      textColor: json.strOrNull('textColor'),
      accentColor: json.strOrNull('accentColor'),
      secondaryTextColor: json.strOrNull('secondaryTextColor'),
      padding: (json['padding'] as num?)?.toDouble(),
      margin: (json['margin'] as num?)?.toDouble(),
      borderRadius: (json['borderRadius'] as num?)?.toDouble(),
      borderWidth: (json['borderWidth'] as num?)?.toDouble(),
      borderStyle: json.strOrNull('borderStyle'),
      flexDirection: json.strOrNull('flexDirection'),
      justifyContent: json.strOrNull('justifyContent'),
      alignItems: json.strOrNull('alignItems'),
      height: json['height'],
      width: json['width'],
      fontFamily: json.strOrNull('fontFamily'),
      fontSize: json.strOrNull('fontSize'),
      textAlign: json.strOrNull('textAlign'),
      offsetX: (json['offsetX'] as num?)?.toDouble(),
      offsetY: (json['offsetY'] as num?)?.toDouble(),
    );
  }

  SanitizedStyles copyWith({
    String? backgroundColor,
    String? borderColor,
    String? textColor,
    String? accentColor,
    String? secondaryTextColor,
    double? padding,
    double? margin,
    double? borderRadius,
    double? borderWidth,
    String? borderStyle,
    String? flexDirection,
    String? justifyContent,
    String? alignItems,
    dynamic height,
    dynamic width,
    String? fontFamily,
    String? fontSize,
    String? textAlign,
    double? offsetX,
    double? offsetY,
  }) {
    return SanitizedStyles(
      backgroundColor: backgroundColor ?? this.backgroundColor,
      borderColor: borderColor ?? this.borderColor,
      textColor: textColor ?? this.textColor,
      accentColor: accentColor ?? this.accentColor,
      secondaryTextColor: secondaryTextColor ?? this.secondaryTextColor,
      padding: padding ?? this.padding,
      margin: margin ?? this.margin,
      borderRadius: borderRadius ?? this.borderRadius,
      borderWidth: borderWidth ?? this.borderWidth,
      borderStyle: borderStyle ?? this.borderStyle,
      flexDirection: flexDirection ?? this.flexDirection,
      justifyContent: justifyContent ?? this.justifyContent,
      alignItems: alignItems ?? this.alignItems,
      height: height ?? this.height,
      width: width ?? this.width,
      fontFamily: fontFamily ?? this.fontFamily,
      fontSize: fontSize ?? this.fontSize,
      textAlign: textAlign ?? this.textAlign,
      offsetX: offsetX ?? this.offsetX,
      offsetY: offsetY ?? this.offsetY,
    );
  }
}

class BaseWidgetProperties {
  final String? title;
  final bool isVisible;

  BaseWidgetProperties({
    this.title,
    this.isVisible = true,
  });

  Map<String, dynamic> toJson() {
    return {
      'title': title,
      'isVisible': isVisible,
    };
  }

  factory BaseWidgetProperties.fromJson(Map<String, dynamic> json) {
    return BaseWidgetProperties(
      title: json.strOrNull('title'),
      isVisible: json['isVisible'] as bool? ?? true,
    );
  }
}

class ThemeProperties extends BaseWidgetProperties {
  final String themeName;
  final String? backgroundImageUrl;
  final double? backgroundBlur;
  final bool? enableOverlay;

  ThemeProperties({
    required this.themeName,
    this.backgroundImageUrl,
    this.backgroundBlur,
    this.enableOverlay,
    super.title,
    super.isVisible,
  });

  @override
  Map<String, dynamic> toJson() {
    final json = super.toJson();
    json['themeName'] = themeName;
    json['backgroundImageUrl'] = backgroundImageUrl;
    json['backgroundBlur'] = backgroundBlur;
    json['enableOverlay'] = enableOverlay;
    return json;
  }

  factory ThemeProperties.fromJson(Map<String, dynamic> json) {
    return ThemeProperties(
      themeName: json['themeName'] as String,
      backgroundImageUrl: json.strOrNull('backgroundImageUrl'),
      backgroundBlur: (json['backgroundBlur'] as num?)?.toDouble(),
      enableOverlay: json['enableOverlay'] as bool?,
      title: json.strOrNull('title'),
      isVisible: json['isVisible'] as bool? ?? true,
    );
  }
}

class TextBlockProperties extends BaseWidgetProperties {
  final String content;
  final bool markdownEnabled;

  TextBlockProperties({
    required this.content,
    this.markdownEnabled = false,
    super.title,
    super.isVisible,
  });

  @override
  Map<String, dynamic> toJson() {
    final json = super.toJson();
    json['content'] = content;
    json['markdownEnabled'] = markdownEnabled;
    return json;
  }

  factory TextBlockProperties.fromJson(Map<String, dynamic> json) {
    return TextBlockProperties(
      content: json.strOf('content'),
      markdownEnabled: json.boolOf('markdownEnabled'),
      title: json.strOrNull('title'),
      isVisible: json['isVisible'] as bool? ?? true,
    );
  }
}

class ProfileMediaItem {
  final String id;
  final String url;
  final String type;
  final String? caption;
  final String? content;

  ProfileMediaItem({
    required this.id,
    required this.url,
    required this.type,
    this.caption,
    this.content,
  });

  Map<String, dynamic> toJson() {
    return {
      'id': id,
      'url': url,
      'type': type,
      'caption': caption,
      'content': content,
    };
  }

  factory ProfileMediaItem.fromJson(Map<String, dynamic> json) {
    return ProfileMediaItem(
      id: json['id'] as String,
      url: json['url'] as String,
      type: json['type'] as String,
      caption: json.strOrNull('caption'),
      content: json.strOrNull('content'),
    );
  }
}

class MediaGalleryProperties extends BaseWidgetProperties {
  final List<ProfileMediaItem> items;
  final String layoutType;
  final int? columns;

  MediaGalleryProperties({
    required this.items,
    this.layoutType = 'grid',
    this.columns,
    super.title,
    super.isVisible,
  });

  @override
  Map<String, dynamic> toJson() {
    final json = super.toJson();
    json['items'] = items.map((e) => e.toJson()).toList();
    json['layoutType'] = layoutType;
    json['columns'] = columns;
    return json;
  }

  factory MediaGalleryProperties.fromJson(Map<String, dynamic> json) {
    return MediaGalleryProperties(
      items: (json['items'] as List<dynamic>?)
              ?.map((e) => ProfileMediaItem.fromJson(e as Map<String, dynamic>))
              .toList() ??
          [],
      layoutType: json.strOrNull('layoutType') ?? 'grid',
      columns: json['columns'] as int?,
      title: json.strOrNull('title'),
      isVisible: json['isVisible'] as bool? ?? true,
    );
  }
}

class FriendGridProperties extends BaseWidgetProperties {
  final int limit;
  final List<String>? customOrder;
  final bool showOnlineStatus;

  FriendGridProperties({
    required this.limit,
    this.customOrder,
    this.showOnlineStatus = true,
    super.title,
    super.isVisible,
  });

  @override
  Map<String, dynamic> toJson() {
    final json = super.toJson();
    json['limit'] = limit;
    json['customOrder'] = customOrder;
    json['showOnlineStatus'] = showOnlineStatus;
    return json;
  }

  factory FriendGridProperties.fromJson(Map<String, dynamic> json) {
    return FriendGridProperties(
      limit: json['limit'] as int? ?? 8,
      customOrder: (json['customOrder'] as List<dynamic>?)
          ?.map((e) => e as String)
          .toList(),
      showOnlineStatus: json['showOnlineStatus'] as bool? ?? true,
      title: json.strOrNull('title'),
      isVisible: json['isVisible'] as bool? ?? true,
    );
  }
}

class AudioTrack {
  final String id;
  final String title;
  final String artist;
  final String url;
  final int? durationSeconds;

  AudioTrack({
    required this.id,
    required this.title,
    required this.artist,
    required this.url,
    this.durationSeconds,
  });

  Map<String, dynamic> toJson() {
    return {
      'id': id,
      'title': title,
      'artist': artist,
      'url': url,
      'durationSeconds': durationSeconds,
    };
  }

  factory AudioTrack.fromJson(Map<String, dynamic> json) {
    return AudioTrack(
      id: json['id'] as String,
      title: json['title'] as String,
      artist: json['artist'] as String,
      url: json['url'] as String,
      durationSeconds: json['durationSeconds'] as int?,
    );
  }
}

class MusicPlayerProperties extends BaseWidgetProperties {
  final List<AudioTrack> tracks;
  final bool autoplay;
  final bool loop;

  MusicPlayerProperties({
    required this.tracks,
    this.autoplay = false,
    this.loop = false,
    super.title,
    super.isVisible,
  });

  @override
  Map<String, dynamic> toJson() {
    final json = super.toJson();
    json['tracks'] = tracks.map((e) => e.toJson()).toList();
    json['autoplay'] = autoplay;
    json['loop'] = loop;
    return json;
  }

  factory MusicPlayerProperties.fromJson(Map<String, dynamic> json) {
    return MusicPlayerProperties(
      tracks: (json['tracks'] as List<dynamic>?)
              ?.map((e) => AudioTrack.fromJson(e as Map<String, dynamic>))
              .toList() ??
          [],
      autoplay: json.boolOf('autoplay'),
      loop: json.boolOf('loop'),
      title: json.strOrNull('title'),
      isVisible: json['isVisible'] as bool? ?? true,
    );
  }
}

class ContactCardProperties extends BaseWidgetProperties {
  final bool enableMessage;
  final bool enableVouch;
  final bool enableAddFriend;
  final List<Map<String, String>>? customLinks;

  ContactCardProperties({
    this.enableMessage = true,
    this.enableVouch = false,
    this.enableAddFriend = true,
    this.customLinks,
    super.title,
    super.isVisible,
  });

  @override
  Map<String, dynamic> toJson() {
    final json = super.toJson();
    json['enableMessage'] = enableMessage;
    json['enableVouch'] = enableVouch;
    json['enableAddFriend'] = enableAddFriend;
    json['customLinks'] = customLinks;
    return json;
  }

  factory ContactCardProperties.fromJson(Map<String, dynamic> json) {
    return ContactCardProperties(
      enableMessage: json['enableMessage'] as bool? ?? true,
      enableVouch: json.boolOf('enableVouch'),
      enableAddFriend: json['enableAddFriend'] as bool? ?? true,
      customLinks: (json['customLinks'] as List<dynamic>?)
          ?.map((e) => Map<String, String>.from(e as Map))
          .toList(),
      title: json.strOrNull('title'),
      isVisible: json['isVisible'] as bool? ?? true,
    );
  }
}

class QAPair {
  final String question;
  final String answer;

  QAPair({
    required this.question,
    required this.answer,
  });

  Map<String, dynamic> toJson() {
    return {
      'question': question,
      'answer': answer,
    };
  }

  factory QAPair.fromJson(Map<String, dynamic> json) {
    return QAPair(
      question: json['question'] as String,
      answer: json['answer'] as String,
    );
  }
}

class QAListProperties extends BaseWidgetProperties {
  final List<QAPair> pairs;

  QAListProperties({
    required this.pairs,
    super.title,
    super.isVisible,
  });

  @override
  Map<String, dynamic> toJson() {
    final json = super.toJson();
    json['pairs'] = pairs.map((e) => e.toJson()).toList();
    return json;
  }

  factory QAListProperties.fromJson(Map<String, dynamic> json) {
    return QAListProperties(
      pairs: (json['pairs'] as List<dynamic>?)
              ?.map((e) => QAPair.fromJson(e as Map<String, dynamic>))
              .toList() ??
          [],
      title: json.strOrNull('title'),
      isVisible: json['isVisible'] as bool? ?? true,
    );
  }
}

class TabDef {
  final String id;
  final String label;
  final List<ProfileMediaItem> items;

  TabDef({
    required this.id,
    required this.label,
    required this.items,
  });

  Map<String, dynamic> toJson() {
    return {
      'id': id,
      'label': label,
      'items': items.map((e) => e.toJson()).toList(),
    };
  }

  factory TabDef.fromJson(Map<String, dynamic> json) {
    return TabDef(
      id: json['id'] as String,
      label: json['label'] as String,
      items: (json['items'] as List<dynamic>?)
              ?.map((e) => ProfileMediaItem.fromJson(e as Map<String, dynamic>))
              .toList() ??
          [],
    );
  }
}

class TabContainerProperties extends BaseWidgetProperties {
  final List<TabDef> tabs;

  TabContainerProperties({
    required this.tabs,
    super.title,
    super.isVisible,
  });

  @override
  Map<String, dynamic> toJson() {
    final json = super.toJson();
    json['tabs'] = tabs.map((e) => e.toJson()).toList();
    return json;
  }

  factory TabContainerProperties.fromJson(Map<String, dynamic> json) {
    return TabContainerProperties(
      tabs: (json['tabs'] as List<dynamic>?)
              ?.map((e) => TabDef.fromJson(e as Map<String, dynamic>))
              .toList() ??
          [],
      title: json.strOrNull('title'),
      isVisible: json['isVisible'] as bool? ?? true,
    );
  }
}

class GuestbookEntry {
  final String id;
  final String pubkey;
  final String name;
  final String? avatar;
  final String content;
  final int createdAt;
  final String? sig;
  final bool? approved;

  GuestbookEntry({
    required this.id,
    required this.pubkey,
    required this.name,
    this.avatar,
    required this.content,
    required this.createdAt,
    this.sig,
    this.approved,
  });

  Map<String, dynamic> toJson() {
    return {
      'id': id,
      'pubkey': pubkey,
      'name': name,
      'avatar': avatar,
      'content': content,
      'createdAt': createdAt,
      'sig': sig,
      'approved': approved,
    };
  }

  factory GuestbookEntry.fromJson(Map<String, dynamic> json) {
    return GuestbookEntry(
      id: json['id'] as String,
      pubkey: json['pubkey'] as String,
      name: json['name'] as String,
      avatar: json.strOrNull('avatar'),
      content: json['content'] as String,
      createdAt: json['createdAt'] as int,
      sig: json.strOrNull('sig'),
      approved: json['approved'] as bool?,
    );
  }
}

class GuestbookProperties extends BaseWidgetProperties {
  final List<GuestbookEntry> entries;
  final bool allowAnonymous;
  final int maxEntries;

  GuestbookProperties({
    required this.entries,
    this.allowAnonymous = false,
    this.maxEntries = 20,
    super.title,
    super.isVisible,
  });

  @override
  Map<String, dynamic> toJson() {
    final json = super.toJson();
    json['entries'] = entries.map((e) => e.toJson()).toList();
    json['allowAnonymous'] = allowAnonymous;
    json['maxEntries'] = maxEntries;
    return json;
  }

  factory GuestbookProperties.fromJson(Map<String, dynamic> json) {
    return GuestbookProperties(
      entries: (json['entries'] as List<dynamic>?)
              ?.map((e) => GuestbookEntry.fromJson(e as Map<String, dynamic>))
              .toList() ??
          [],
      allowAnonymous: json.boolOf('allowAnonymous'),
      maxEntries: json['maxEntries'] as int? ?? 20,
      title: json.strOrNull('title'),
      isVisible: json['isVisible'] as bool? ?? true,
    );
  }
}

class ProfileLinksProperties extends BaseWidgetProperties {
  final bool showMinis;
  final bool showMusicloud;

  ProfileLinksProperties({
    this.showMinis = true,
    this.showMusicloud = true,
    super.title,
    super.isVisible,
  });

  @override
  Map<String, dynamic> toJson() {
    final json = super.toJson();
    json['showMinis'] = showMinis;
    json['showMusicloud'] = showMusicloud;
    return json;
  }

  factory ProfileLinksProperties.fromJson(Map<String, dynamic> json) {
    return ProfileLinksProperties(
      showMinis: json['showMinis'] as bool? ?? true,
      showMusicloud: json['showMusicloud'] as bool? ?? true,
      title: json.strOrNull('title'),
      isVisible: json['isVisible'] as bool? ?? true,
    );
  }
}

class HistoryProperties extends BaseWidgetProperties {
  final bool showReposts;
  final int maxEntries;

  HistoryProperties({
    this.showReposts = true,
    this.maxEntries = 50,
    super.title,
    super.isVisible,
  });

  @override
  Map<String, dynamic> toJson() {
    final json = super.toJson();
    json['showReposts'] = showReposts;
    json['maxEntries'] = maxEntries;
    return json;
  }

  factory HistoryProperties.fromJson(Map<String, dynamic> json) {
    return HistoryProperties(
      showReposts: json['showReposts'] as bool? ?? true,
      maxEntries: json['maxEntries'] as int? ?? 50,
      title: json.strOrNull('title'),
      isVisible: json['isVisible'] as bool? ?? true,
    );
  }
}

class NodePosition {
  final int row;
  final int column;
  final int order;

  NodePosition({
    required this.row,
    required this.column,
    required this.order,
  });

  Map<String, dynamic> toJson() {
    return {
      'row': row,
      'column': column,
      'order': order,
    };
  }

  factory NodePosition.fromJson(Map<String, dynamic> json) {
    return NodePosition(
      row: json['row'] as int? ?? 0,
      column: json['column'] as int? ?? 0,
      order: json['order'] as int? ?? 0,
    );
  }

  NodePosition copyWith({
    int? row,
    int? column,
    int? order,
  }) {
    return NodePosition(
      row: row ?? this.row,
      column: column ?? this.column,
      order: order ?? this.order,
    );
  }
}

class CustomProfileNode {
  final String id;
  final String type;
  final SanitizedStyles styles;
  final NodePosition position;
  final Map<String, dynamic> properties;

  CustomProfileNode({
    required this.id,
    required this.type,
    required this.styles,
    required this.position,
    required this.properties,
  });

  Map<String, dynamic> toJson() {
    return {
      'id': id,
      'type': type,
      'styles': styles.toJson(),
      'position': position.toJson(),
      'properties': properties,
    };
  }

  factory CustomProfileNode.fromJson(Map<String, dynamic> json) {
    return CustomProfileNode(
      id: json.strOf('id'),
      type: json.strOf('type'),
      styles: SanitizedStyles.fromJson(
          Map<String, dynamic>.from(json['styles'] as Map? ?? const {})),
      position: NodePosition.fromJson(
          Map<String, dynamic>.from(json['position'] as Map? ?? const {})),
      properties:
          Map<String, dynamic>.from(json['properties'] as Map? ?? const {}),
    );
  }

  CustomProfileNode copyWith({
    String? id,
    String? type,
    SanitizedStyles? styles,
    NodePosition? position,
    Map<String, dynamic>? properties,
  }) {
    return CustomProfileNode(
      id: id ?? this.id,
      type: type ?? this.type,
      styles: styles ?? this.styles,
      position: position ?? this.position,
      properties: properties ?? this.properties,
    );
  }
}

class CustomProfile {
  final String themeId;
  final List<CustomProfileNode> nodes;

  CustomProfile({
    required this.themeId,
    required this.nodes,
  });

  Map<String, dynamic> toJson() {
    return {
      'themeId': themeId,
      'nodes': nodes.map((e) => e.toJson()).toList(),
    };
  }

  factory CustomProfile.fromJson(Map<String, dynamic> json) {
    return CustomProfile(
      themeId: json.strOrNull('themeId') ?? 'default',
      nodes: (json['nodes'] as List<dynamic>?)
              ?.map(
                  (e) => CustomProfileNode.fromJson(e as Map<String, dynamic>))
              .toList() ??
          [],
    );
  }
}

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

/// Owns custom-profile operations: DB load/save, schema authority (node
/// types + defaults live in Rust), npub encoding for QR.
/// Legal FFI-call site per architecture rules; screens must use this
/// instead of calling glue fns directly.
class ProfileService extends ChangeNotifier {
  List<NodeTypeInfo> _nodeTypes = const [];

  /// The 12 supported node types, sourced from Rust (schema authority).
  List<NodeTypeInfo> get nodeTypes => _nodeTypes;

  ProfileService() {
    _loadNodeTypes();
  }

  void _loadNodeTypes() {
    try {
      final json = ffi_content.contentCustomProfileNodeTypes();
      final list = (jsonDecode(json) as List<dynamic>)
          .map((e) => NodeTypeInfo(
                type: (e as Map<String, dynamic>)['type'] as String,
                label: e['label'] as String,
                icon: e['icon'] as String,
              ))
          .toList();
      _nodeTypes = List.unmodifiable(list);
    } catch (e) {
      debugPrint('Failed to load node types from Rust: $e');
      _nodeTypes = const [];
    }
  }

  /// Default node JSON for a node type; schema defaults live in Rust.
  String defaultNode({required String type, required int index}) =>
      ffi_content.contentCustomProfileDefaultNode(
        nodeType: type,
        index: BigInt.from(index),
      );

  /// Strict-validate a profile payload; returns canonical JSON or throws.
  String validateProfile(String profileJson) =>
      ffi_content.contentCustomProfileValidate(
        profileJson: profileJson,
      );

  /// Load custom profile nodes JSON for a pubkey from the database.
  String getCustomProfileNodes({required String pubkey}) =>
      ffi_db.dbGetCustomProfileNodes(pubkey: pubkey);

  /// Save custom profile JSON for a pubkey to the database.
  bool saveCustomProfile({
    required String pubkey,
    required String profileJson,
  }) =>
      ffi_db.dbSaveCustomProfile(pubkey: pubkey, profileJson: profileJson);

  /// Encode a hex public key as bech32 npub (used by QR share).
  String npubEncode({required String publicKey}) =>
      authNpubEncode(publicKey: publicKey);

  /// Sign, store, and relay a guestbook entry for a profile (kind 30080).
  Future<String> guestbookAdd({
    required String profilePubkey,
    required String content,
  }) async {
    return RustLib.instance.api.crateFfiGuestbookGuestbookAdd(
      profilePubkey: profilePubkey,
      content: content,
    );
  }

  /// List guestbook entries for a profile pubkey from the local DB.
  String guestbookList({
    required String profilePubkey,
    required int limit,
    required bool onlyApproved,
  }) =>
      RustLib.instance.api.crateFfiGuestbookGuestbookList(
        profilePubkey: profilePubkey,
        limit: limit,
        onlyApproved: onlyApproved,
      );

  /// Owner approval/denial of a guestbook entry (kind 30081).
  Future<String> guestbookApprove({
    required String entryId,
    required bool approved,
  }) async {
    return RustLib.instance.api.crateFfiGuestbookGuestbookApprove(
      entryId: entryId,
      approved: approved,
    );
  }

  /// Owner-only local delete of a guestbook entry.
  bool guestbookDelete({required String entryId}) =>
      RustLib.instance.api.crateFfiGuestbookGuestbookDelete(
        entryId: entryId,
      );
}
