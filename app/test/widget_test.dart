import 'package:flutter_test/flutter_test.dart';

import 'package:phone_upload/main.dart';

void main() {
  testWidgets('shows the receiver page', (WidgetTester tester) async {
    await tester.pumpWidget(const PhoneDropApp());
    expect(find.text('Phone Drop'), findsOneWidget);
  });
}
