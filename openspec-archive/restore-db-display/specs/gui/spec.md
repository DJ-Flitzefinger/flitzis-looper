## MODIFIED Requirements

### Requirement: Visible dB Display Above Gain Slider
The Volume +EQ dialog SHALL display a dB value indicator above the gain slider that is clearly visible to users.

#### Scenario: Initial dialog display
Given the Volume +EQ dialog is opened
When the dialog is first displayed
Then a dB value label showing the current gain setting shall be visible above the slider

#### Scenario: Slider movement updates dB display
Given the Volume +EQ dialog is open
When the user moves the gain slider
Then the dB value label shall update to show the new value
And the label shall move to follow the slider thumb position

#### Scenario: dB display visibility
Given the Volume +EQ dialog is displayed
When viewing the dialog
Then the dB value label shall be clearly readable against the background
And the label shall not be obscured by other UI elements