pub use flexbuffers;

use flexbuffers::{BitWidth, DeserializationError, FlexBufferType, ReaderError, ReaderIterator};
use serde::{
	de::{
		DeserializeOwned, DeserializeSeed, EnumAccess, IntoDeserializer, MapAccess, SeqAccess,
		VariantAccess, Visitor,
	},
	ser::{
		SerializeMap, SerializeSeq, SerializeStruct, SerializeStructVariant, SerializeTuple,
		SerializeTupleStruct, SerializeTupleVariant,
	},
	Deserialize, Deserializer, Serialize, Serializer,
};
use std::{borrow::BorrowMut, cell::RefCell, fmt::Display, marker::PhantomData};

#[derive(Debug, thiserror::Error)]
pub enum FlexSerializeError {
	#[error("Map key is not a string!")]
	MapKeyNotString,
	#[error("custom")]
	Serde(String),
}
impl serde::ser::Error for FlexSerializeError {
	fn custom<T>(msg: T) -> Self
	where
		T: Display,
	{
		FlexSerializeError::Serde(msg.to_string())
	}
}

/// Serialize the given data into flexbuffers, stripping struct field names off
/// and putting structs into vectors to save space and computation.
pub fn serialize<S: Serialize>(to_serialize: S) -> Result<Vec<u8>, FlexSerializeError> {
	let mut fbb = flexbuffers::Builder::default();
	let fs = FlexSerializer { fbb: &mut fbb };
	to_serialize.serialize(fs)?;
	Ok(fbb.take_buffer())
}

struct FlexSerializer<'b> {
	fbb: &'b mut flexbuffers::Builder,
}
impl<'b> Serializer for FlexSerializer<'b> {
	type Ok = ();
	type Error = FlexSerializeError;

	type SerializeSeq = FlexVecSerializer<'b>;
	type SerializeTuple = FlexVecSerializer<'b>;
	type SerializeTupleStruct = FlexVecSerializer<'b>;
	type SerializeTupleVariant = FlexVecSerializer<'b>;
	type SerializeMap = FlexMapSerializer<'b>;
	type SerializeStruct = FlexVecSerializer<'b>;
	type SerializeStructVariant = FlexVecSerializer<'b>;

	fn serialize_bool(self, v: bool) -> Result<Self::Ok, FlexSerializeError> {
		self.fbb.build_singleton(v);
		Ok(())
	}
	fn serialize_i8(self, v: i8) -> Result<Self::Ok, FlexSerializeError> {
		self.fbb.build_singleton(v);
		Ok(())
	}

	fn serialize_i16(self, v: i16) -> Result<Self::Ok, FlexSerializeError> {
		self.fbb.build_singleton(v);
		Ok(())
	}

	fn serialize_i32(self, v: i32) -> Result<Self::Ok, FlexSerializeError> {
		self.fbb.build_singleton(v);
		Ok(())
	}

	fn serialize_i64(self, v: i64) -> Result<Self::Ok, FlexSerializeError> {
		self.fbb.build_singleton(v);
		Ok(())
	}

	fn serialize_u8(self, v: u8) -> Result<Self::Ok, FlexSerializeError> {
		self.fbb.build_singleton(v);
		Ok(())
	}

	fn serialize_u16(self, v: u16) -> Result<Self::Ok, FlexSerializeError> {
		self.fbb.build_singleton(v);
		Ok(())
	}

	fn serialize_u32(self, v: u32) -> Result<Self::Ok, FlexSerializeError> {
		self.fbb.build_singleton(v);
		Ok(())
	}

	fn serialize_u64(self, v: u64) -> Result<Self::Ok, FlexSerializeError> {
		self.fbb.build_singleton(v);
		Ok(())
	}

	fn serialize_f32(self, v: f32) -> Result<Self::Ok, FlexSerializeError> {
		self.fbb.build_singleton(v);
		Ok(())
	}

	fn serialize_f64(self, v: f64) -> Result<Self::Ok, FlexSerializeError> {
		self.fbb.build_singleton(v);
		Ok(())
	}

	fn serialize_char(self, v: char) -> Result<Self::Ok, FlexSerializeError> {
		self.fbb.build_singleton(v.to_string().as_str());
		Ok(())
	}

	fn serialize_str(self, v: &str) -> Result<Self::Ok, FlexSerializeError> {
		self.fbb.build_singleton(v);
		Ok(())
	}

	fn serialize_bytes(self, v: &[u8]) -> Result<Self::Ok, FlexSerializeError> {
		self.fbb.build_singleton(v);
		Ok(())
	}

	fn serialize_none(self) -> Result<Self::Ok, FlexSerializeError> {
		self.fbb.build_singleton(());
		Ok(())
	}

	fn serialize_some<T: ?Sized>(self, value: &T) -> Result<Self::Ok, FlexSerializeError>
	where
		T: Serialize,
	{
		value.serialize(self)
	}

	fn serialize_unit(self) -> Result<Self::Ok, FlexSerializeError> {
		self.fbb.build_singleton(());
		Ok(())
	}

	fn serialize_unit_struct(self, _name: &'static str) -> Result<Self::Ok, FlexSerializeError> {
		self.serialize_unit()
	}

	fn serialize_unit_variant(
		self,
		_name: &'static str,
		variant_index: u32,
		_variant: &'static str,
	) -> Result<Self::Ok, FlexSerializeError> {
		self.serialize_u32(variant_index)
	}

	fn serialize_newtype_struct<T: ?Sized>(
		self,
		_name: &'static str,
		value: &T,
	) -> Result<Self::Ok, FlexSerializeError>
	where
		T: Serialize,
	{
		value.serialize(self)
	}

	fn serialize_newtype_variant<T: ?Sized>(
		self,
		_name: &'static str,
		_variant_index: u32,
		_variant: &'static str,
		value: &T,
	) -> Result<Self::Ok, FlexSerializeError>
	where
		T: Serialize,
	{
		// TODO: actually store variant type too
		value.serialize(self)
	}

	fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq, FlexSerializeError> {
		Ok(FlexVecSerializer {
			fvb: self.fbb.start_vector(),
		})
	}

	fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple, FlexSerializeError> {
		Ok(FlexVecSerializer {
			fvb: self.fbb.start_vector(),
		})
	}

	fn serialize_tuple_struct(
		self,
		_name: &'static str,
		_len: usize,
	) -> Result<Self::SerializeTupleStruct, FlexSerializeError> {
		Ok(FlexVecSerializer {
			fvb: self.fbb.start_vector(),
		})
	}

	fn serialize_tuple_variant(
		self,
		_name: &'static str,
		_variant_index: u32,
		_variant: &'static str,
		_len: usize,
	) -> Result<Self::SerializeTupleVariant, FlexSerializeError> {
		// TODO: actually store variant type too
		Ok(FlexVecSerializer {
			fvb: self.fbb.start_vector(),
		})
	}

	fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap, FlexSerializeError> {
		Ok(FlexMapSerializer {
			fmb: self.fbb.start_map(),
			key: FlexMapKeySerializer {
				key: String::new(),
				phantom: PhantomData::default(),
			},
		})
	}

	fn serialize_struct(
		self,
		_name: &'static str,
		_len: usize,
	) -> Result<Self::SerializeStruct, FlexSerializeError> {
		Ok(FlexVecSerializer {
			fvb: self.fbb.start_vector(),
		})
	}

	fn serialize_struct_variant(
		self,
		_name: &'static str,
		_variant_index: u32,
		_variant: &'static str,
		_len: usize,
	) -> Result<Self::SerializeStructVariant, FlexSerializeError> {
		// TODO: actually store variant type too
		Ok(FlexVecSerializer {
			fvb: self.fbb.start_vector(),
		})
	}
}

struct FlexVecSerializer<'b> {
	fvb: flexbuffers::VectorBuilder<'b>,
}

struct FlexVecSerializerWrapper<'parent, 'fvb>(&'fvb mut flexbuffers::VectorBuilder<'parent>);

impl<'parent, 'fvb> Serializer for FlexVecSerializerWrapper<'parent, 'fvb> {
	type Ok = ();
	type Error = FlexSerializeError;

	type SerializeSeq = FlexVecSerializer<'fvb>;
	type SerializeTuple = FlexVecSerializer<'fvb>;
	type SerializeTupleStruct = FlexVecSerializer<'fvb>;
	type SerializeTupleVariant = FlexVecSerializer<'fvb>;
	type SerializeMap = FlexMapSerializer<'fvb>;
	type SerializeStruct = FlexVecSerializer<'fvb>;
	type SerializeStructVariant = FlexVecSerializer<'fvb>;

	fn serialize_bool(self, v: bool) -> Result<Self::Ok, FlexSerializeError> {
		self.0.push(v);
		Ok(())
	}
	fn serialize_i8(self, v: i8) -> Result<Self::Ok, FlexSerializeError> {
		self.0.push(v);
		Ok(())
	}

	fn serialize_i16(self, v: i16) -> Result<Self::Ok, FlexSerializeError> {
		self.0.push(v);
		Ok(())
	}

	fn serialize_i32(self, v: i32) -> Result<Self::Ok, FlexSerializeError> {
		self.0.push(v);
		Ok(())
	}

	fn serialize_i64(self, v: i64) -> Result<Self::Ok, FlexSerializeError> {
		self.0.push(v);
		Ok(())
	}

	fn serialize_u8(self, v: u8) -> Result<Self::Ok, FlexSerializeError> {
		self.0.push(v);
		Ok(())
	}

	fn serialize_u16(self, v: u16) -> Result<Self::Ok, FlexSerializeError> {
		self.0.push(v);
		Ok(())
	}

	fn serialize_u32(self, v: u32) -> Result<Self::Ok, FlexSerializeError> {
		self.0.push(v);
		Ok(())
	}

	fn serialize_u64(self, v: u64) -> Result<Self::Ok, FlexSerializeError> {
		self.0.push(v);
		Ok(())
	}

	fn serialize_f32(self, v: f32) -> Result<Self::Ok, FlexSerializeError> {
		self.0.push(v);
		Ok(())
	}

	fn serialize_f64(self, v: f64) -> Result<Self::Ok, FlexSerializeError> {
		self.0.push(v);
		Ok(())
	}

	fn serialize_char(self, v: char) -> Result<Self::Ok, FlexSerializeError> {
		self.0.push(v.to_string().as_str());
		Ok(())
	}

	fn serialize_str(self, v: &str) -> Result<Self::Ok, FlexSerializeError> {
		self.0.push(v);
		Ok(())
	}

	fn serialize_bytes(self, v: &[u8]) -> Result<Self::Ok, FlexSerializeError> {
		self.0.push(v);
		Ok(())
	}

	fn serialize_none(self) -> Result<Self::Ok, FlexSerializeError> {
		self.0.push(());
		Ok(())
	}

	fn serialize_some<T: ?Sized>(self, value: &T) -> Result<Self::Ok, FlexSerializeError>
	where
		T: Serialize,
	{
		value.serialize(self)
	}

	fn serialize_unit(self) -> Result<Self::Ok, FlexSerializeError> {
		self.0.push(());
		Ok(())
	}

	fn serialize_unit_struct(self, _name: &'static str) -> Result<Self::Ok, FlexSerializeError> {
		self.serialize_unit()
	}

	fn serialize_unit_variant(
		self,
		_name: &'static str,
		variant_index: u32,
		_variant: &'static str,
	) -> Result<Self::Ok, FlexSerializeError> {
		self.serialize_u32(variant_index)
	}

	fn serialize_newtype_struct<T: ?Sized>(
		self,
		_name: &'static str,
		value: &T,
	) -> Result<Self::Ok, FlexSerializeError>
	where
		T: Serialize,
	{
		value.serialize(self)
	}

	fn serialize_newtype_variant<T: ?Sized>(
		self,
		_name: &'static str,
		_variant_index: u32,
		_variant: &'static str,
		value: &T,
	) -> Result<Self::Ok, FlexSerializeError>
	where
		T: Serialize,
	{
		// TODO: actually store variant type too
		value.serialize(self)
	}

	fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq, FlexSerializeError> {
		Ok(FlexVecSerializer {
			fvb: self.0.start_vector(),
		})
	}

	fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple, FlexSerializeError> {
		Ok(FlexVecSerializer {
			fvb: self.0.start_vector(),
		})
	}

	fn serialize_tuple_struct(
		self,
		_name: &'static str,
		_len: usize,
	) -> Result<Self::SerializeTupleStruct, FlexSerializeError> {
		Ok(FlexVecSerializer {
			fvb: self.0.start_vector(),
		})
	}

	fn serialize_tuple_variant(
		self,
		_name: &'static str,
		_variant_index: u32,
		_variant: &'static str,
		_len: usize,
	) -> Result<Self::SerializeTupleVariant, FlexSerializeError> {
		// TODO: actually store variant type too
		Ok(FlexVecSerializer {
			fvb: self.0.start_vector(),
		})
	}

	fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap, FlexSerializeError> {
		Ok(FlexMapSerializer {
			fmb: self.0.start_map(),
			key: FlexMapKeySerializer {
				key: String::new(),
				phantom: PhantomData::default(),
			},
		})
	}

	fn serialize_struct(
		self,
		_name: &'static str,
		_len: usize,
	) -> Result<Self::SerializeStruct, FlexSerializeError> {
		Ok(FlexVecSerializer {
			fvb: self.0.start_vector(),
		})
	}

	fn serialize_struct_variant(
		self,
		_name: &'static str,
		_variant_index: u32,
		_variant: &'static str,
		_len: usize,
	) -> Result<Self::SerializeStructVariant, FlexSerializeError> {
		// TODO: actually store variant type too
		Ok(FlexVecSerializer {
			fvb: self.0.start_vector(),
		})
	}
}
impl<'b> SerializeSeq for FlexVecSerializer<'b> {
	type Ok = ();
	type Error = FlexSerializeError;

	fn serialize_element<T: ?Sized>(&mut self, value: &T) -> Result<(), FlexSerializeError>
	where
		T: Serialize,
	{
		value.serialize(FlexVecSerializerWrapper(&mut self.fvb))
	}

	fn end(self) -> Result<Self::Ok, FlexSerializeError> {
		Ok(())
	}
}
impl<'b> SerializeTuple for FlexVecSerializer<'b> {
	type Ok = ();
	type Error = FlexSerializeError;

	fn serialize_element<T: ?Sized>(&mut self, value: &T) -> Result<(), FlexSerializeError>
	where
		T: Serialize,
	{
		value.serialize(FlexVecSerializerWrapper(&mut self.fvb))
	}

	fn end(self) -> Result<Self::Ok, FlexSerializeError> {
		Ok(())
	}
}
impl<'b> SerializeTupleStruct for FlexVecSerializer<'b> {
	type Ok = ();
	type Error = FlexSerializeError;

	fn serialize_field<T: ?Sized>(&mut self, value: &T) -> Result<(), FlexSerializeError>
	where
		T: Serialize,
	{
		value.serialize(FlexVecSerializerWrapper(&mut self.fvb))
	}

	fn end(self) -> Result<Self::Ok, FlexSerializeError> {
		Ok(())
	}
}
impl<'b> SerializeTupleVariant for FlexVecSerializer<'b> {
	type Ok = ();
	type Error = FlexSerializeError;

	fn serialize_field<T: ?Sized>(&mut self, value: &T) -> Result<(), FlexSerializeError>
	where
		T: Serialize,
	{
		value.serialize(FlexVecSerializerWrapper(&mut self.fvb))
	}

	fn end(self) -> Result<Self::Ok, FlexSerializeError> {
		Ok(())
	}
}
impl<'b> SerializeStruct for FlexVecSerializer<'b> {
	type Ok = ();
	type Error = FlexSerializeError;

	fn serialize_field<T: ?Sized>(
		&mut self,
		_key: &'static str,
		value: &T,
	) -> Result<(), FlexSerializeError>
	where
		T: Serialize,
	{
		value.serialize(FlexVecSerializerWrapper(&mut self.fvb))
	}

	fn end(self) -> Result<Self::Ok, FlexSerializeError> {
		Ok(())
	}
}

impl<'b> SerializeStructVariant for FlexVecSerializer<'b> {
	type Ok = ();
	type Error = FlexSerializeError;

	fn serialize_field<T: ?Sized>(
		&mut self,
		_key: &'static str,
		value: &T,
	) -> Result<(), FlexSerializeError>
	where
		T: Serialize,
	{
		value.serialize(FlexVecSerializerWrapper(&mut self.fvb))
	}

	fn end(self) -> Result<Self::Ok, FlexSerializeError> {
		Ok(())
	}
}

struct FlexMapKeySerializer<'b> {
	key: String,
	phantom: PhantomData<&'b ()>,
}
impl<'b> Serializer for &mut FlexMapKeySerializer<'b> {
	type Ok = ();
	type Error = FlexSerializeError;

	type SerializeSeq = FlexVecSerializer<'b>;
	type SerializeTuple = FlexVecSerializer<'b>;
	type SerializeTupleStruct = FlexVecSerializer<'b>;
	type SerializeTupleVariant = FlexVecSerializer<'b>;
	type SerializeMap = FlexMapSerializer<'b>;
	type SerializeStruct = FlexVecSerializer<'b>;
	type SerializeStructVariant = FlexVecSerializer<'b>;

	fn serialize_bool(self, _v: bool) -> Result<Self::Ok, FlexSerializeError> {
		Err(FlexSerializeError::MapKeyNotString)
	}

	fn serialize_i8(self, _v: i8) -> Result<Self::Ok, FlexSerializeError> {
		Err(FlexSerializeError::MapKeyNotString)
	}

	fn serialize_i16(self, _v: i16) -> Result<Self::Ok, FlexSerializeError> {
		Err(FlexSerializeError::MapKeyNotString)
	}

	fn serialize_i32(self, _v: i32) -> Result<Self::Ok, FlexSerializeError> {
		Err(FlexSerializeError::MapKeyNotString)
	}

	fn serialize_i64(self, _v: i64) -> Result<Self::Ok, FlexSerializeError> {
		Err(FlexSerializeError::MapKeyNotString)
	}

	fn serialize_u8(self, _v: u8) -> Result<Self::Ok, FlexSerializeError> {
		Err(FlexSerializeError::MapKeyNotString)
	}

	fn serialize_u16(self, _v: u16) -> Result<Self::Ok, FlexSerializeError> {
		Err(FlexSerializeError::MapKeyNotString)
	}

	fn serialize_u32(self, _v: u32) -> Result<Self::Ok, FlexSerializeError> {
		Err(FlexSerializeError::MapKeyNotString)
	}

	fn serialize_u64(self, _v: u64) -> Result<Self::Ok, FlexSerializeError> {
		Err(FlexSerializeError::MapKeyNotString)
	}

	fn serialize_f32(self, _v: f32) -> Result<Self::Ok, FlexSerializeError> {
		Err(FlexSerializeError::MapKeyNotString)
	}

	fn serialize_f64(self, _v: f64) -> Result<Self::Ok, FlexSerializeError> {
		Err(FlexSerializeError::MapKeyNotString)
	}

	fn serialize_char(self, v: char) -> Result<Self::Ok, FlexSerializeError> {
		self.key = v.to_string();
		Ok(())
	}

	fn serialize_str(self, v: &str) -> Result<Self::Ok, FlexSerializeError> {
		self.key = v.to_string();
		Ok(())
	}

	fn serialize_bytes(self, _v: &[u8]) -> Result<Self::Ok, FlexSerializeError> {
		Err(FlexSerializeError::MapKeyNotString)
	}

	fn serialize_none(self) -> Result<Self::Ok, FlexSerializeError> {
		Err(FlexSerializeError::MapKeyNotString)
	}

	fn serialize_some<T: ?Sized>(self, _value: &T) -> Result<Self::Ok, FlexSerializeError>
	where
		T: Serialize,
	{
		Err(FlexSerializeError::MapKeyNotString)
	}

	fn serialize_unit(self) -> Result<Self::Ok, FlexSerializeError> {
		Err(FlexSerializeError::MapKeyNotString)
	}

	fn serialize_unit_struct(self, _name: &'static str) -> Result<Self::Ok, FlexSerializeError> {
		Err(FlexSerializeError::MapKeyNotString)
	}

	fn serialize_unit_variant(
		self,
		_name: &'static str,
		_variant_index: u32,
		_variant: &'static str,
	) -> Result<Self::Ok, FlexSerializeError> {
		Err(FlexSerializeError::MapKeyNotString)
	}

	fn serialize_newtype_struct<T: ?Sized>(
		self,
		_name: &'static str,
		_value: &T,
	) -> Result<Self::Ok, FlexSerializeError>
	where
		T: Serialize,
	{
		Err(FlexSerializeError::MapKeyNotString)
	}

	fn serialize_newtype_variant<T: ?Sized>(
		self,
		_name: &'static str,
		_variant_index: u32,
		_variant: &'static str,
		_value: &T,
	) -> Result<Self::Ok, FlexSerializeError>
	where
		T: Serialize,
	{
		Err(FlexSerializeError::MapKeyNotString)
	}

	fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq, FlexSerializeError> {
		Err(FlexSerializeError::MapKeyNotString)
	}

	fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple, FlexSerializeError> {
		Err(FlexSerializeError::MapKeyNotString)
	}

	fn serialize_tuple_struct(
		self,
		_name: &'static str,
		_len: usize,
	) -> Result<Self::SerializeTupleStruct, FlexSerializeError> {
		Err(FlexSerializeError::MapKeyNotString)
	}

	fn serialize_tuple_variant(
		self,
		_name: &'static str,
		_variant_index: u32,
		_variant: &'static str,
		_len: usize,
	) -> Result<Self::SerializeTupleVariant, FlexSerializeError> {
		Err(FlexSerializeError::MapKeyNotString)
	}

	fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap, FlexSerializeError> {
		Err(FlexSerializeError::MapKeyNotString)
	}

	fn serialize_struct(
		self,
		_name: &'static str,
		_len: usize,
	) -> Result<Self::SerializeStruct, FlexSerializeError> {
		Err(FlexSerializeError::MapKeyNotString)
	}

	fn serialize_struct_variant(
		self,
		_name: &'static str,
		_variant_index: u32,
		_variant: &'static str,
		_len: usize,
	) -> Result<Self::SerializeStructVariant, FlexSerializeError> {
		Err(FlexSerializeError::MapKeyNotString)
	}
}

struct FlexMapSerializer<'b> {
	fmb: flexbuffers::MapBuilder<'b>,
	key: FlexMapKeySerializer<'b>,
}

struct FlexMapSerializerWrapper<'parent, 'fvb>(
	&'fvb mut FlexMapKeySerializer<'parent>,
	&'fvb mut flexbuffers::MapBuilder<'parent>,
);

impl<'parent, 'fvb> Serializer for FlexMapSerializerWrapper<'parent, 'fvb> {
	type Ok = ();
	type Error = FlexSerializeError;

	type SerializeSeq = FlexVecSerializer<'fvb>;
	type SerializeTuple = FlexVecSerializer<'fvb>;
	type SerializeTupleStruct = FlexVecSerializer<'fvb>;
	type SerializeTupleVariant = FlexVecSerializer<'fvb>;
	type SerializeMap = FlexMapSerializer<'fvb>;
	type SerializeStruct = FlexVecSerializer<'fvb>;
	type SerializeStructVariant = FlexVecSerializer<'fvb>;

	fn serialize_bool(self, v: bool) -> Result<Self::Ok, FlexSerializeError> {
		self.1.push(&self.0.key, v);
		Ok(())
	}
	fn serialize_i8(self, v: i8) -> Result<Self::Ok, FlexSerializeError> {
		self.1.push(&self.0.key, v);
		Ok(())
	}

	fn serialize_i16(self, v: i16) -> Result<Self::Ok, FlexSerializeError> {
		self.1.push(&self.0.key, v);
		Ok(())
	}

	fn serialize_i32(self, v: i32) -> Result<Self::Ok, FlexSerializeError> {
		self.1.push(&self.0.key, v);
		Ok(())
	}

	fn serialize_i64(self, v: i64) -> Result<Self::Ok, FlexSerializeError> {
		self.1.push(&self.0.key, v);
		Ok(())
	}

	fn serialize_u8(self, v: u8) -> Result<Self::Ok, FlexSerializeError> {
		self.1.push(&self.0.key, v);
		Ok(())
	}

	fn serialize_u16(self, v: u16) -> Result<Self::Ok, FlexSerializeError> {
		self.1.push(&self.0.key, v);
		Ok(())
	}

	fn serialize_u32(self, v: u32) -> Result<Self::Ok, FlexSerializeError> {
		self.1.push(&self.0.key, v);
		Ok(())
	}

	fn serialize_u64(self, v: u64) -> Result<Self::Ok, FlexSerializeError> {
		self.1.push(&self.0.key, v);
		Ok(())
	}

	fn serialize_f32(self, v: f32) -> Result<Self::Ok, FlexSerializeError> {
		self.1.push(&self.0.key, v);
		Ok(())
	}

	fn serialize_f64(self, v: f64) -> Result<Self::Ok, FlexSerializeError> {
		self.1.push(&self.0.key, v);
		Ok(())
	}

	fn serialize_char(self, v: char) -> Result<Self::Ok, FlexSerializeError> {
		self.1.push(&self.0.key, v.to_string().as_str());
		Ok(())
	}

	fn serialize_str(self, v: &str) -> Result<Self::Ok, FlexSerializeError> {
		self.1.push(&self.0.key, v);
		Ok(())
	}

	fn serialize_bytes(self, v: &[u8]) -> Result<Self::Ok, FlexSerializeError> {
		self.1.push(&self.0.key, v);
		Ok(())
	}

	fn serialize_none(self) -> Result<Self::Ok, FlexSerializeError> {
		self.1.push(&self.0.key, ());
		Ok(())
	}

	fn serialize_some<T: ?Sized>(self, value: &T) -> Result<Self::Ok, FlexSerializeError>
	where
		T: Serialize,
	{
		value.serialize(self)
	}

	fn serialize_unit(self) -> Result<Self::Ok, FlexSerializeError> {
		self.1.push(&self.0.key, ());
		Ok(())
	}

	fn serialize_unit_struct(self, _name: &'static str) -> Result<Self::Ok, FlexSerializeError> {
		self.serialize_unit()
	}

	fn serialize_unit_variant(
		self,
		_name: &'static str,
		variant_index: u32,
		_variant: &'static str,
	) -> Result<Self::Ok, FlexSerializeError> {
		self.serialize_u32(variant_index)
	}

	fn serialize_newtype_struct<T: ?Sized>(
		self,
		_name: &'static str,
		value: &T,
	) -> Result<Self::Ok, FlexSerializeError>
	where
		T: Serialize,
	{
		value.serialize(self)
	}

	fn serialize_newtype_variant<T: ?Sized>(
		self,
		_name: &'static str,
		_variant_index: u32,
		_variant: &'static str,
		value: &T,
	) -> Result<Self::Ok, FlexSerializeError>
	where
		T: Serialize,
	{
		// TODO: actually store variant type too
		value.serialize(self)
	}

	fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq, FlexSerializeError> {
		Ok(FlexVecSerializer {
			fvb: self.1.start_vector(&self.0.key),
		})
	}

	fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple, FlexSerializeError> {
		Ok(FlexVecSerializer {
			fvb: self.1.start_vector(&self.0.key),
		})
	}

	fn serialize_tuple_struct(
		self,
		_name: &'static str,
		_len: usize,
	) -> Result<Self::SerializeTupleStruct, FlexSerializeError> {
		Ok(FlexVecSerializer {
			fvb: self.1.start_vector(&self.0.key),
		})
	}

	fn serialize_tuple_variant(
		self,
		_name: &'static str,
		_variant_index: u32,
		_variant: &'static str,
		_len: usize,
	) -> Result<Self::SerializeTupleVariant, FlexSerializeError> {
		// TODO: actually store variant type too
		Ok(FlexVecSerializer {
			fvb: self.1.start_vector(&self.0.key),
		})
	}

	fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap, FlexSerializeError> {
		Ok(FlexMapSerializer {
			fmb: self.1.start_map(&self.0.key),
			key: FlexMapKeySerializer {
				key: String::new(),
				phantom: PhantomData::default(),
			},
		})
	}

	fn serialize_struct(
		self,
		_name: &'static str,
		_len: usize,
	) -> Result<Self::SerializeStruct, FlexSerializeError> {
		Ok(FlexVecSerializer {
			fvb: self.1.start_vector(&self.0.key),
		})
	}

	fn serialize_struct_variant(
		self,
		_name: &'static str,
		_variant_index: u32,
		_variant: &'static str,
		_len: usize,
	) -> Result<Self::SerializeStructVariant, FlexSerializeError> {
		// TODO: actually store variant type too
		Ok(FlexVecSerializer {
			fvb: self.1.start_vector(&self.0.key),
		})
	}
}

impl<'b> SerializeMap for FlexMapSerializer<'b> {
	type Ok = ();
	type Error = FlexSerializeError;

	fn serialize_key<T: ?Sized>(&mut self, key: &T) -> Result<(), FlexSerializeError>
	where
		T: Serialize,
	{
		key.serialize(&mut self.key)
	}

	fn serialize_value<T: ?Sized>(&mut self, value: &T) -> Result<(), FlexSerializeError>
	where
		T: Serialize,
	{
		value.serialize(FlexMapSerializerWrapper(&mut self.key, &mut self.fmb))
	}

	fn end(self) -> Result<Self::Ok, FlexSerializeError> {
		Ok(())
	}
}

/// Deserialize given flexbuffers data into whatever format.
/// This is different than the regular flexbuffers deserialization
/// because it strips the names off of struct fields,
/// instead putting the values into vectors with the same
/// order to save space and speed up deserialization.
pub fn deserialize<'a, T: Deserialize<'a>>(data: &'a [u8]) -> Result<T, DeserializationError> {
	let root = flexbuffers::Reader::get_root(data)?;
	let deserializer = FlexbuffersDeserializer(root);
	T::deserialize(deserializer)
}

/// Deserialize given flexbuffers data into whatever format.
/// This is different than the regular flexbuffers deserialization
/// because it strips the names off of struct fields,
/// instead putting the values into vectors with the same
/// order to save space and speed up deserialization.
pub fn deserialize_owned<T: DeserializeOwned>(data: Vec<u8>) -> Result<T, DeserializationError> {
	let root = flexbuffers::Reader::get_root(data.as_slice())?;
	let deserializer = FlexbuffersDeserializer(root);
	T::deserialize(deserializer)
}

struct ReaderIteratorWrapper<'d>(ReaderIterator<&'d [u8]>);

impl<'de> SeqAccess<'de> for ReaderIteratorWrapper<'de> {
	type Error = DeserializationError;

	fn next_element_seed<T>(
		&mut self,
		seed: T,
	) -> Result<Option<<T as DeserializeSeed<'de>>::Value>, Self::Error>
	where
		T: DeserializeSeed<'de>,
	{
		if let Some(elem) = self.0.next() {
			seed.deserialize(elem).map(Some)
		} else {
			Ok(None)
		}
	}

	fn size_hint(&self) -> Option<usize> {
		Some(self.0.len())
	}
}

struct EnumReader<'de> {
	variant: &'de str,
	value: Option<flexbuffers::Reader<&'de [u8]>>,
}

impl<'de> EnumAccess<'de> for EnumReader<'de> {
	type Error = DeserializationError;
	type Variant = FlexbuffersDeserializer<'de>;

	fn variant_seed<V>(self, seed: V) -> Result<(V::Value, Self::Variant), Self::Error>
	where
		V: DeserializeSeed<'de>,
	{
		seed.deserialize(self.variant.into_deserializer())
			.map(|v| (v, FlexbuffersDeserializer(self.value.unwrap_or_default())))
	}
}

struct MapAccessor<'de> {
	keys: ReaderIteratorWrapper<'de>,
	vals: ReaderIteratorWrapper<'de>,
}

impl<'de> MapAccess<'de> for MapAccessor<'de> {
	type Error = DeserializationError;

	fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error>
	where
		K: DeserializeSeed<'de>,
	{
		if let Some(k) = self.keys.0.next() {
			seed.deserialize(k).map(Some)
		} else {
			Ok(None)
		}
	}

	fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, Self::Error>
	where
		V: DeserializeSeed<'de>,
	{
		let val = self.vals.0.next().ok_or(ReaderError::IndexOutOfBounds)?;
		seed.deserialize(val)
	}
}

struct FlexbuffersDeserializer<'de>(flexbuffers::Reader<&'de [u8]>);

impl<'de> VariantAccess<'de> for FlexbuffersDeserializer<'de> {
	type Error = DeserializationError;

	fn unit_variant(self) -> Result<(), Self::Error> {
		Ok(())
	}

	fn newtype_variant_seed<T>(self, seed: T) -> Result<T::Value, Self::Error>
	where
		T: DeserializeSeed<'de>,
	{
		seed.deserialize(self)
	}

	fn tuple_variant<V>(self, _len: usize, visitor: V) -> Result<V::Value, Self::Error>
	where
		V: Visitor<'de>,
	{
		visitor.visit_seq(self.0.get_vector()?.iter())
	}

	fn struct_variant<V>(
		self,
		_fields: &'static [&'static str],
		visitor: V,
	) -> Result<V::Value, Self::Error>
	where
		V: Visitor<'de>,
	{
		visitor.visit_seq(self.0.get_vector()?.iter())
	}
}

impl<'de> Deserializer<'de> for FlexbuffersDeserializer<'de> {
	type Error = DeserializationError;

	fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
	where
		V: Visitor<'de>,
	{
		use BitWidth::*;
		use FlexBufferType::*;
		match (self.0.flexbuffer_type(), self.0.bitwidth()) {
			(Bool, _) => visitor.visit_bool(self.0.as_bool()),
			(UInt, bw) => match bw {
				W8 => visitor.visit_u8(self.0.as_u8()),
				W16 => visitor.visit_u16(self.0.as_u16()),
				W32 => visitor.visit_u32(self.0.as_u32()),
				W64 => visitor.visit_u64(self.0.as_u64()),
			},
			(Int, bw) => match bw {
				W8 => visitor.visit_i8(self.0.as_i8()),
				W16 => visitor.visit_i16(self.0.as_i16()),
				W32 => visitor.visit_i32(self.0.as_i32()),
				W64 => visitor.visit_i64(self.0.as_i64()),
			},
			(Float, bw) => {
				match bw {
					W32 => visitor.visit_f32(self.0.as_f32()),
					W64 => visitor.visit_f64(self.0.as_f64()),
					_ => Err(ReaderError::InvalidPackedType.into()), // f8 and f16 are not supported.
				}
			}
			(Null, _) => visitor.visit_unit(),
			(String, _) | (Key, _) => visitor.visit_borrowed_str(self.0.get_str()?),
			(Blob, _) => visitor.visit_borrowed_bytes(self.0.get_blob()?.0),
			(Map, _) => {
				let m = self.0.get_map()?;
				visitor.visit_map(MapAccessor {
					keys: ReaderIteratorWrapper(m.keys_vector().iter()),
					vals: ReaderIteratorWrapper(m.iter_values()),
				})
			}
			(ty, _) if ty.is_vector() => visitor.visit_seq(self.0.get_vector()?.iter()),
			(ty, bw) => unreachable!("TODO deserialize_any {:?} {:?}.", ty, bw),
		}
	}
	serde::forward_to_deserialize_any! {
		bool i8 i16 i32 i64 u8 u16 u32 u64 f32 f64 str unit unit_struct bytes
		ignored_any map identifier struct tuple tuple_struct seq string
	}
	fn deserialize_char<V>(self, visitor: V) -> Result<V::Value, Self::Error>
	where
		V: Visitor<'de>,
	{
		visitor.visit_char(self.0.get_u64()? as u8 as char)
	}

	fn deserialize_byte_buf<V>(self, visitor: V) -> Result<V::Value, Self::Error>
	where
		V: Visitor<'de>,
	{
		visitor.visit_byte_buf(self.0.get_blob()?.0.to_vec())
	}

	fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Self::Error>
	where
		V: Visitor<'de>,
	{
		if self.0.flexbuffer_type() == FlexBufferType::Null {
			visitor.visit_none()
		} else {
			visitor.visit_some(self)
		}
	}

	fn deserialize_newtype_struct<V>(
		self,
		_name: &'static str,
		visitor: V,
	) -> Result<V::Value, Self::Error>
	where
		V: Visitor<'de>,
	{
		visitor.visit_newtype_struct(self)
	}

	fn deserialize_enum<V>(
		self,
		_name: &'static str,
		_variants: &'static [&'static str],
		visitor: V,
	) -> Result<V::Value, Self::Error>
	where
		V: Visitor<'de>,
	{
		let (variant, value) = match self.0.flexbuffer_type() {
			FlexBufferType::String => (self.0.as_str(), None),
			FlexBufferType::Vector => {
				let m = self.0.get_vector()?;
				let variant = "t";
				let value = Some(m.idx(0));
				(variant, value)
			}
			FlexBufferType::Map => {
				let m = self.0.get_map()?;
				let variant = m.keys_vector().idx(0).get_key()?;
				let value = Some(m.idx(0));
				(variant, value)
			}
			_ => {
				return Err(flexbuffers::ReaderError::UnexpectedFlexbufferType {
					expected: FlexBufferType::Map,
					actual: self.0.flexbuffer_type(),
				}
				.into());
			}
		};
		visitor.visit_enum(EnumReader { variant, value })
	}
}

#[test]
fn round_trip_flex_serialize() {
	use mint::{Quaternion, Vector2, Vector3};
	use serde::{Deserialize, Serialize};
	use serde_repr::{Deserialize_repr, Serialize_repr};
	#[derive(Debug, PartialEq, Clone, Serialize_repr, Deserialize_repr)]
	#[repr(u32)]
	enum TestEnum {
		Value1,
		Value2,
	}
	#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
	struct TestStruct {
		b: bool,
		i1: i8,
		i2: i16,
		i3: i32,
		i4: i64,
		u1: u8,
		u2: u16,
		u3: u32,
		u4: u64,
		f1: f32,
		f2: f64,
		test_vec2: Vector2<f32>,
		test_vec3: Vector3<f32>,
		test_quat: Quaternion<f32>,
		string: String,
		test_enum: TestEnum,
		test_struct: Option<Box<TestStruct>>,
	}

	let mut test_struct = TestStruct {
		b: true,
		i1: 25,
		i2: 25,
		i3: 25,
		i4: 25,
		u1: 25,
		u2: 25,
		u3: 25,
		u4: 25,
		f1: 0.5,
		f2: 0.5,
		test_vec2: Vector2::from([0.7; 2]),
		test_vec3: Vector3::from([0.63; 3]),
		test_quat: Quaternion {
			v: Vector3::from([3.14; 3]),
			s: 12.0,
		},
		string: "Test Test".to_string(),
		test_enum: TestEnum::Value1,
		test_struct: None,
	};
	test_struct.test_struct = Some(Box::new(test_struct.clone()));
	let serialized = serialize(test_struct.clone()).unwrap();
	let flex = flexbuffers::Reader::get_root(serialized.as_slice()).unwrap();
	println!("{}", flex);
	let deserialized: TestStruct = deserialize(&serialized).unwrap();
	assert_eq!(test_struct, deserialized, "Round trip lost data");
}
