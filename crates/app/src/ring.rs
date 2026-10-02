//! The circular progress ring around the countdown.

use std::f32::consts::PI;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{gdk, glib, graphene, gsk};

const SIZE: i32 = 220;
const STROKE: f32 = 10.0;

mod imp {
    use std::cell::Cell;

    use super::*;

    #[derive(Default, glib::Properties)]
    #[properties(wrapper_type = super::CountdownRing)]
    pub struct CountdownRing {
        /// How much of the freeze is left, from 0 to 1.
        #[property(get, set = Self::set_fraction, minimum = 0.0, maximum = 1.0)]
        fraction: Cell<f64>,
    }

    impl CountdownRing {
        fn set_fraction(&self, fraction: f64) {
            if (self.fraction.get() - fraction).abs() > f64::EPSILON {
                self.fraction.set(fraction);
                self.obj().queue_draw();
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for CountdownRing {
        const NAME: &'static str = "PermafrostCountdownRing";
        type Type = super::CountdownRing;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_accessible_role(gtk::AccessibleRole::ProgressBar);
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for CountdownRing {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            adw::StyleManager::default().connect_accent_color_rgba_notify(glib::clone!(
                #[weak]
                obj,
                move |_| obj.queue_draw()
            ));
        }
    }

    impl WidgetImpl for CountdownRing {
        fn measure(&self, _orientation: gtk::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            (SIZE, SIZE, -1, -1)
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let obj = self.obj();
            let (w, h) = (obj.width() as f32, obj.height() as f32);
            let radius = w.min(h) / 2.0 - STROKE / 2.0;
            let center = graphene::Point::new(w / 2.0, h / 2.0);
            let stroke = gsk::Stroke::builder(STROKE).line_cap(gsk::LineCap::Round).build();

            let fg = obj.color();
            let track = gdk::RGBA::new(fg.red(), fg.green(), fg.blue(), 0.1);
            let circle = gsk::PathBuilder::new();
            circle.add_circle(&center, radius);
            snapshot.append_stroke(&circle.to_path(), &stroke, &track);

            let fraction = self.fraction.get() as f32;
            if fraction <= 0.0 {
                return;
            }
            let accent = adw::StyleManager::default().accent_color_rgba();
            let arc = gsk::PathBuilder::new();
            if fraction >= 0.999 {
                arc.add_circle(&center, radius);
            } else {
                // Clockwise from 12 o'clock.
                let angle = fraction * 2.0 * PI;
                arc.move_to(center.x(), center.y() - radius);
                arc.svg_arc_to(
                    radius,
                    radius,
                    0.0,
                    angle > PI,
                    true,
                    center.x() + radius * angle.sin(),
                    center.y() - radius * angle.cos(),
                );
            }
            snapshot.append_stroke(&arc.to_path(), &stroke, &accent);
        }
    }
}

glib::wrapper! {
    pub struct CountdownRing(ObjectSubclass<imp::CountdownRing>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}
