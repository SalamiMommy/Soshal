import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/services/friends_service.dart';
import 'package:soshal_flutter/widgets/audience_filter_dropdown.dart';

void main() {
  testWidgets('AudienceFilterDropdown renders label and icon for initial value',
      (tester) async {
    AudienceFilter selected = AudienceFilter.all;

    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          appBar: AppBar(
            actions: [
              AudienceFilterDropdown(
                value: selected,
                onChanged: (val) => selected = val,
              ),
            ],
          ),
          body: const Center(child: Text('Body')),
        ),
      ),
    );

    // Initial label is 'All' and has public icon
    expect(find.text('All'), findsOneWidget);
    expect(find.byIcon(Icons.public), findsOneWidget);
    expect(find.byIcon(Icons.arrow_drop_down), findsOneWidget);
  });

  testWidgets(
      'AudienceFilterDropdown opens menu and selects Friends of Friends',
      (tester) async {
    AudienceFilter selected = AudienceFilter.all;

    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          appBar: AppBar(
            actions: [
              StatefulBuilder(
                builder: (context, setState) => AudienceFilterDropdown(
                  value: selected,
                  onChanged: (val) {
                    setState(() => selected = val);
                  },
                ),
              ),
            ],
          ),
          body: const Center(child: Text('Body')),
        ),
      ),
    );

    // Tap dropdown button
    await tester.tap(find.byType(AudienceFilterDropdown));
    await tester.pumpAndSettle();

    // Verify all 3 options are displayed in the popup menu
    expect(find.text('Friends of Friends'), findsOneWidget);
    expect(find.text('Friends'), findsOneWidget);
    // 'All' is in both app bar and popup menu
    expect(find.text('All'), findsNWidgets(2));

    // Tap 'Friends of Friends'
    await tester.tap(find.text('Friends of Friends'));
    await tester.pumpAndSettle();

    // Verify selected value changed and updated in the widget
    expect(selected, AudienceFilter.friendsOfFriends);
    expect(find.text('Friends of Friends'), findsOneWidget);
    expect(find.byIcon(Icons.groups_outlined), findsOneWidget);
  });

  testWidgets('AudienceFilterDropdown selects Friends option',
      (tester) async {
    AudienceFilter selected = AudienceFilter.all;

    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          appBar: AppBar(
            actions: [
              StatefulBuilder(
                builder: (context, setState) => AudienceFilterDropdown(
                  value: selected,
                  onChanged: (val) {
                    setState(() => selected = val);
                  },
                ),
              ),
            ],
          ),
        ),
      ),
    );

    // Tap dropdown button
    await tester.tap(find.byType(AudienceFilterDropdown));
    await tester.pumpAndSettle();

    // Tap 'Friends'
    await tester.tap(find.text('Friends'));
    await tester.pumpAndSettle();

    // Verify selected value is Friends
    expect(selected, AudienceFilter.friends);
    expect(find.text('Friends'), findsOneWidget);
    expect(find.byIcon(Icons.people_outline), findsOneWidget);
  });
}
